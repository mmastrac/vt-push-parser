use std::borrow::Cow;

use vt_push_parser::{VT_PARSER_INTEREST_NONE, VTEvent, VTPushParser};

/// Strip ANSI escape sequences from a string. If the output contains no ANSI
/// escape sequences, the input is returned as-is.
pub fn strip_ansi_string(s: &str) -> Cow<str> {
    let mut output = Cow::Borrowed(s);
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_NONE>();
    parser.feed_with(s.as_bytes(), &mut |event| match event {
        VTEvent::Raw(text) => {
            if text.len() == s.len() {
                return;
            }
            let output = match &mut output {
                Cow::Borrowed(_) => {
                    output = Cow::Owned(String::with_capacity(s.len()));
                    let Cow::Owned(s) = &mut output else {
                        unreachable!()
                    };
                    s
                }
                Cow::Owned(s) => s,
            };
            output.push_str(String::from_utf8_lossy(text).as_ref());
        }
        _ => {}
    });
    output
}

/// Strip ANSI escape sequences from a byte slice. If the output contains no
/// ANSI escape sequences, the input is returned as-is.
pub fn strip_ansi_bytes(s: &[u8]) -> Cow<[u8]> {
    let mut output = Cow::Borrowed(s);
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_NONE>();
    parser.feed_with(s, &mut |event| match event {
        VTEvent::Raw(text) => {
            if text.len() == s.len() {
                return;
            }
            let output = match &mut output {
                Cow::Borrowed(_) => {
                    output = Cow::Owned(Vec::with_capacity(s.len()));
                    let Cow::Owned(s) = &mut output else {
                        unreachable!()
                    };
                    s
                }
                Cow::Owned(s) => s,
            };
            output.extend_from_slice(text);
        }
        _ => {}
    });
    output
}

/// Strip ANSI escape sequences from a byte slice, calling a callback for each
/// raw text chunk.
pub fn strip_ansi_bytes_callback(s: &[u8], mut cb: impl FnMut(&[u8])) {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_NONE>();
    parser.feed_with(s, &mut |event| match event {
        VTEvent::Raw(text) => cb(text),
        _ => {}
    });
}

pub struct AnsiStrip {
    parser: VTPushParser<VT_PARSER_INTEREST_NONE>,
}

impl AnsiStrip {
    pub fn new() -> Self {
        Self {
            parser: VTPushParser::new_with_interest::<VT_PARSER_INTEREST_NONE>(),
        }
    }

    pub fn feed(&mut self, s: &[u8], cb: &mut impl FnMut(&[u8])) {
        self.parser.feed_with(s, &mut |event| match event {
            VTEvent::Raw(text) => cb(text),
            _ => {}
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_string() {
        let input = "Hello, world!\x1b[31mHello, world!\x1b[0m";
        let output = strip_ansi_string(input);
        assert_eq!(output, "Hello, world!Hello, world!");
    }

    #[test]
    fn test_strip_ansi_as_is() {
        let input = b"Hello, world!";
        let output = strip_ansi_bytes(input);
        assert_eq!(output, b"Hello, world!".as_slice());
        assert!(matches!(output, Cow::Borrowed(_)));
    }
}
