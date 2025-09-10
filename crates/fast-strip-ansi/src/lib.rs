use std::borrow::Cow;

use vt_push_parser::{VT_PARSER_INTEREST_NONE, VTEvent, VTPushParser};

/// Strip ANSI escape sequences from a string. If the input contains no ANSI
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

/// Strip ANSI escape sequences from a byte slice. If the input contains no
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

/// A streaming ANSI escape sequence stripper that can be fed chunks of data
/// and yields text chunks to a callback.
pub struct StreamingStripper {
    parser: VTPushParser<VT_PARSER_INTEREST_NONE>,
}

impl StreamingStripper {
    pub const fn new() -> Self {
        Self {
            parser: VTPushParser::new_with_interest::<VT_PARSER_INTEREST_NONE>(),
        }
    }

    /// Feed a chunk of data to the stripper. The callback will be called for
    /// each raw text chunk.
    pub fn feed(&mut self, s: &[u8], cb: &mut impl FnMut(&[u8])) {
        self.parser.feed_with(s, &mut |event| match event {
            VTEvent::Raw(text) => cb(text),
            _ => {}
        });
    }
}

/// A writer that strips ANSI escape sequences from the data written to it and
/// feeds the underlying writer with the raw text chunks.
///
/// Due to limitations of the [`std::io::Write`] interface, if the underlying
/// writer returns an error, it may be uncertain how many bytes were written.
pub struct Writer<W: std::io::Write> {
    writer: W,
    ansi_strip: StreamingStripper,
}

impl<W: std::io::Write> Writer<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            ansi_strip: StreamingStripper::new(),
        }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl<W: std::io::Write> std::io::Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        let mut error = None;
        self.ansi_strip.feed(buf, &mut |text| {
            if error.is_none() {
                _ = self.writer.write_all(text).map_err(|e| {
                    error = Some(e);
                });
            }
        });
        if let Some(error) = error.take() {
            Err(error)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

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

    #[test]
    fn test_writer() {
        let mut writer = Writer::new(Vec::new());
        writer
            .write_all(b"Hello, world!\x1b[31mHello, world!\x1b[0m")
            .unwrap();
        assert_eq!(writer.into_inner(), b"Hello, world!Hello, world!");
    }
}
