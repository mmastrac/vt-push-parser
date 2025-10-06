//! Raw-input-capturing push parser.

use crate::{VT_PARSER_INTEREST_DEFAULT, VTEvent, VTPushParser};

/// The type of capture mode to use after this event has been emitted.
///
/// The data will be emitted as a [`VTInputEvent::Captured`] event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VTInputCapture {
    /// No capture mode. This must also be returned from any
    /// [`VTInputEvent::Captured`] event.
    None,
    /// Capture a fixed number of bytes.
    Count(usize),
    /// Capture a fixed number of UTF-8 chars.
    CountUtf8(usize),
    /// Capture bytes until a terminator is found.
    Terminator(&'static [u8]),
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug)]
pub enum VTCaptureEvent<'a> {
    VTEvent(VTEvent<'a>),
    Capture(&'a [u8]),
    CaptureEnd,
}

enum VTCaptureInternal {
    None,
    Count(usize),
    CountUtf8(usize),
    Terminator(&'static [u8], usize),
}

impl VTCaptureInternal {
    fn feed<'a>(&mut self, input: &mut &'a [u8]) -> Option<&'a [u8]> {
        match self {
            VTCaptureInternal::None => None,
            VTCaptureInternal::Count(count) => {
                if input.len() >= *count {
                    let (capture, rest) = input.split_at(*count);
                    *input = rest;
                    *self = VTCaptureInternal::None;
                    Some(capture)
                } else {
                    None
                }
            }
            VTCaptureInternal::CountUtf8(count) => {
                // Count UTF-8 characters, not bytes
                let mut chars_found = 0;
                let mut bytes_consumed = 0;

                for (i, &byte) in input.iter().enumerate() {
                    // Check if this is the start of a new UTF-8 character
                    if byte & 0xC0 != 0x80 {
                        // Not a continuation byte
                        chars_found += 1;
                        if chars_found == *count {
                            // We found the nth character, now we need to find where it ends
                            // by consuming all its continuation bytes
                            let mut j = i + 1;
                            while j < input.len() && input[j] & 0xC0 == 0x80 {
                                j += 1;
                            }
                            bytes_consumed = j;
                            break;
                        }
                    }
                }

                if chars_found == *count {
                    let (capture, rest) = input.split_at(bytes_consumed);
                    *input = rest;
                    *self = VTCaptureInternal::None;
                    Some(capture)
                } else {
                    None
                }
            }
            VTCaptureInternal::Terminator(terminator, found) => {
                // Ground state
                if *found == 0 {
                    if let Some(position) = input.iter().position(|&b| b == terminator[0]) {
                        // Advance to first match position
                        *found = 1;
                        let unmatched = &input[..position];
                        *input = &input[position + 1..];
                        return Some(unmatched);
                    } else {
                        let unmatched = *input;
                        *input = &[];
                        return Some(unmatched);
                    }
                }

                // We've already found part of the terminator, so we can continue
                while *found < terminator.len() {
                    if input.is_empty() {
                        return None;
                    }

                    if input[0] == terminator[*found] {
                        *found += 1;
                        *input = &input[1..];
                    } else {
                        // Failed a match, so return the part of the terminator we already matched
                        let old_found = std::mem::take(found);
                        return Some(&terminator[..old_found]);
                    }
                }

                // We've matched the entire terminator
                *self = VTCaptureInternal::None;
                None
            }
        }
    }
}

/// A parser that allows for "capturing" of input data, ie: temporarily
/// transferring control of the parser to unparsed data events.
///
/// This functions in the same way as [`VTPushParser`], but emits
/// [`VTCaptureEvent`]s instead of [`VTEvent`]s.
pub struct VTCapturePushParser<const INTEREST: u8 = VT_PARSER_INTEREST_DEFAULT> {
    parser: VTPushParser<INTEREST>,
    capture: VTCaptureInternal,
}

impl Default for VTCapturePushParser {
    fn default() -> Self {
        Self::new()
    }
}

impl VTCapturePushParser {
    pub const fn new() -> VTCapturePushParser {
        VTCapturePushParser::new_with_interest::<VT_PARSER_INTEREST_DEFAULT>()
    }

    pub const fn new_with_interest<const INTEREST: u8>() -> VTCapturePushParser<INTEREST> {
        VTCapturePushParser::new_with()
    }
}

impl<const INTEREST: u8> VTCapturePushParser<INTEREST> {
    const fn new_with() -> Self {
        Self {
            parser: VTPushParser::new_with(),
            capture: VTCaptureInternal::None,
        }
    }

    pub fn is_ground(&self) -> bool {
        self.parser.is_ground()
    }

    pub fn idle(&mut self) -> Option<VTCaptureEvent<'static>> {
        self.parser.idle().map(VTCaptureEvent::VTEvent)
    }

    pub fn feed_with<'this, 'input, F: for<'any> FnMut(VTCaptureEvent<'any>) -> VTInputCapture>(
        &'this mut self,
        mut input: &'input [u8],
        cb: &mut F,
    ) {
        while !input.is_empty() {
            match &mut self.capture {
                VTCaptureInternal::None => {
                    // Normal parsing mode - feed to the underlying parser
                    let count = self.parser.feed_with_abortable(input, &mut |event| {
                        let capture_mode = cb(VTCaptureEvent::VTEvent(event));
                        match capture_mode {
                            VTInputCapture::None => {
                                // Stay in normal mode
                            }
                            VTInputCapture::Count(count) => {
                                self.capture = VTCaptureInternal::Count(count);
                            }
                            VTInputCapture::CountUtf8(count) => {
                                self.capture = VTCaptureInternal::CountUtf8(count);
                            }
                            VTInputCapture::Terminator(terminator) => {
                                self.capture = VTCaptureInternal::Terminator(terminator, 0);
                            }
                        }
                        false // Don't abort parsing
                    });

                    input = &input[count..];
                }
                capture => {
                    // Capture mode - collect data until capture is complete
                    if let Some(captured_data) = capture.feed(&mut input) {
                        cb(VTCaptureEvent::Capture(captured_data));
                    }

                    // Check if capture is complete
                    if matches!(self.capture, VTCaptureInternal::None) {
                        cb(VTCaptureEvent::CaptureEnd);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_paste() {
        let mut output = String::new();
        let mut parser = VTCapturePushParser::new();
        parser.feed_with(b"raw\x1b[200~paste\x1b[201~raw", &mut |event| {
            output.push_str(&format!("{event:?}\n"));
            match event {
                VTCaptureEvent::VTEvent(VTEvent::Csi(csi)) => {
                    if csi.params.try_parse::<usize>(0).unwrap_or(0) == 200 {
                        VTInputCapture::Terminator(b"\x1b[201~")
                    } else {
                        VTInputCapture::None
                    }
                }
                _ => VTInputCapture::None,
            }
        });
        assert_eq!(
            output.trim(),
            r#"
VTEvent(Raw('raw'))
VTEvent(Csi('200', '', '~'))
Capture([112, 97, 115, 116, 101])
CaptureEnd
VTEvent(Raw('raw'))
"#
            .trim()
        );
    }

    #[test]
    fn test_capture_count() {
        let mut output = String::new();
        let mut parser = VTCapturePushParser::new();
        parser.feed_with(b"raw\x1b[Xpaste\x1b[Yraw", &mut |event| {
            output.push_str(&format!("{event:?}\n"));
            match event {
                VTCaptureEvent::VTEvent(VTEvent::Csi(csi)) => {
                    if csi.final_byte == b'X' {
                        VTInputCapture::Count(5)
                    } else {
                        VTInputCapture::None
                    }
                }
                _ => VTInputCapture::None,
            }
        });
        assert_eq!(
            output.trim(),
            r#"
VTEvent(Raw('raw'))
VTEvent(Csi('', 'X'))
Capture([112, 97, 115, 116, 101])
CaptureEnd
VTEvent(Csi('', 'Y'))
VTEvent(Raw('raw'))
"#
            .trim()
        );
    }

    #[test]
    fn test_capture_count_utf8_but_ascii() {
        let mut output = String::new();
        let mut parser = VTCapturePushParser::new();
        parser.feed_with(b"raw\x1b[Xpaste\x1b[Yraw", &mut |event| {
            output.push_str(&format!("{event:?}\n"));
            match event {
                VTCaptureEvent::VTEvent(VTEvent::Csi(csi)) => {
                    if csi.final_byte == b'X' {
                        VTInputCapture::CountUtf8(5)
                    } else {
                        VTInputCapture::None
                    }
                }
                _ => VTInputCapture::None,
            }
        });
        assert_eq!(
            output.trim(),
            r#"
VTEvent(Raw('raw'))
VTEvent(Csi('', 'X'))
Capture([112, 97, 115, 116, 101])
CaptureEnd
VTEvent(Csi('', 'Y'))
VTEvent(Raw('raw'))
"#
            .trim()
        );
    }

    #[test]
    fn test_capture_count_utf8() {
        let mut output = String::new();
        let mut parser = VTCapturePushParser::new();
        let input = "raw\u{001b}[X🤖🦕✅😀🕓\u{001b}[Yraw".as_bytes();
        parser.feed_with(input, &mut |event| {
            output.push_str(&format!("{event:?}\n"));
            match event {
                VTCaptureEvent::VTEvent(VTEvent::Csi(csi)) => {
                    if csi.final_byte == b'X' {
                        VTInputCapture::CountUtf8(5)
                    } else {
                        VTInputCapture::None
                    }
                }
                _ => VTInputCapture::None,
            }
        });
        assert_eq!(output.trim(), r#"
VTEvent(Raw('raw'))
VTEvent(Csi('', 'X'))
Capture([240, 159, 164, 150, 240, 159, 166, 149, 226, 156, 133, 240, 159, 152, 128, 240, 159, 149, 147])
CaptureEnd
VTEvent(Csi('', 'Y'))
VTEvent(Raw('raw'))
"#.trim());
    }

    #[test]
    fn test_capture_terminator_partial_match() {
        let mut output = String::new();
        let mut parser = VTCapturePushParser::new();

        parser.feed_with(b"start\x1b[200~part\x1b[201ial\x1b[201~end", &mut |event| {
            output.push_str(&format!("{event:?}\n"));
            match event {
                VTCaptureEvent::VTEvent(VTEvent::Csi(csi)) => {
                    if csi.final_byte == b'~'
                        && csi.params.try_parse::<usize>(0).unwrap_or(0) == 200
                    {
                        VTInputCapture::Terminator(b"\x1b[201~")
                    } else {
                        VTInputCapture::None
                    }
                }
                _ => VTInputCapture::None,
            }
        });

        assert_eq!(
            output.trim(),
            r#"VTEvent(Raw('start'))
VTEvent(Csi('200', '', '~'))
Capture([112, 97, 114, 116])
Capture([27, 91, 50, 48, 49])
Capture([105, 97, 108])
CaptureEnd
VTEvent(Raw('end'))"#
        );
    }

    #[test]
    fn test_capture_terminator_partial_match_single_byte() {
        let input = b"start\x1b[200~part\x1b[201ial\x1b[201~end";

        for chunk_size in 1..5 {
            let (captured, output) = capture_chunk_size(input, chunk_size);
            assert_eq!(captured, b"part\x1b[201ial", "{output}",);
        }
    }

    fn capture_chunk_size(input: &'static [u8; 32], chunk_size: usize) -> (Vec<u8>, String) {
        let mut output = String::new();
        let mut parser = VTCapturePushParser::new();
        let mut captured = Vec::new();
        for chunk in input.chunks(chunk_size) {
            parser.feed_with(chunk, &mut |event| {
                output.push_str(&format!("{event:?}\n"));
                match event {
                    VTCaptureEvent::Capture(data) => {
                        captured.extend_from_slice(data);
                        VTInputCapture::None
                    }
                    VTCaptureEvent::VTEvent(VTEvent::Csi(csi)) => {
                        if csi.final_byte == b'~'
                            && csi.params.try_parse::<usize>(0).unwrap_or(0) == 200
                        {
                            VTInputCapture::Terminator(b"\x1b[201~")
                        } else {
                            VTInputCapture::None
                        }
                    }
                    _ => VTInputCapture::None,
                }
            });
        }
        (captured, output)
    }
}
