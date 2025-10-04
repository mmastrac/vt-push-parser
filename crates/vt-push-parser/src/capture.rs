use crate::{VTEvent, VTPushParser};

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
    Terminator(&'static [u8]),
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
            VTCaptureInternal::Terminator(terminator) => {
                // Search for the terminator sequence
                if let Some(pos) = input
                    .windows(terminator.len())
                    .position(|window| window == *terminator)
                {
                    let (capture, rest) = input.split_at(pos);
                    *input = &rest[terminator.len()..]; // Skip the terminator
                    *self = VTCaptureInternal::None;
                    Some(capture)
                } else {
                    None
                }
            }
        }
    }
}

/// A parser that allows for "capturing" of input data, ie: temporarily
/// transferring control of the parser to unparsed data events.
///
/// This functions in the same way as [`VTPushParser`], but emits
/// [`VTCaptureEvent`]s instead of [`VTEvent`]s.
pub struct VTCapturePushParser {
    parser: VTPushParser,
    capture: VTCaptureInternal,
}

impl VTCapturePushParser {
    pub fn new() -> Self {
        Self {
            parser: VTPushParser::new(),
            capture: VTCaptureInternal::None,
        }
    }

    pub fn is_ground(&self) -> bool {
        self.parser.is_ground()
    }

    pub fn idle(&mut self) -> Option<VTCaptureEvent<'static>> {
        self.parser
            .idle()
            .map(|event| VTCaptureEvent::VTEvent(event))
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
                                self.capture = VTCaptureInternal::Terminator(terminator);
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
            output.push_str(&format!("{:?}\n", event));
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
VTEvent(Csi(, '200', '', '~'))
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
            output.push_str(&format!("{:?}\n", event));
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
VTEvent(Csi(, '', 'X'))
Capture([112, 97, 115, 116, 101])
CaptureEnd
VTEvent(Csi(, '', 'Y'))
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
            output.push_str(&format!("{:?}\n", event));
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
VTEvent(Csi(, '', 'X'))
Capture([112, 97, 115, 116, 101])
CaptureEnd
VTEvent(Csi(, '', 'Y'))
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
            output.push_str(&format!("{:?}\n", event));
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
VTEvent(Csi(, '', 'X'))
Capture([240, 159, 164, 150, 240, 159, 166, 149, 226, 156, 133, 240, 159, 152, 128, 240, 159, 149, 147])
CaptureEnd
VTEvent(Csi(, '', 'Y'))
VTEvent(Raw('raw'))
"#.trim());
    }
}
