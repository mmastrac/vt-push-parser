//! ASCII control codes.

macro_rules! ascii_control {
    ($(($variant:ident, $value:expr)),* $(,)?) => {
        /// ASCII control codes.
        #[repr(u8)]
        pub enum AsciiControl {
            $( $variant = $value, )*
        }

        impl std::fmt::Display for AsciiControl {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( AsciiControl::$variant => write!(f, "<{}>", stringify!($variant).to_ascii_uppercase()), )*
                }
            }
        }

        impl std::fmt::Debug for AsciiControl {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( AsciiControl::$variant => write!(f, "<{}>", stringify!($variant).to_ascii_uppercase()), )*
                }
            }
        }

        impl TryFrom<u8> for AsciiControl {
            type Error = ();
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                $(
                    if value == $value {
                        return Ok(AsciiControl::$variant);
                    }
                )*
                Err(())
            }
        }

        impl TryFrom<char> for AsciiControl {
            type Error = ();
            fn try_from(value: char) -> Result<Self, Self::Error> {
                $(
                    if value == char::from($value) {
                        return Ok(AsciiControl::$variant);
                    }
                )*
                Err(())
            }
        }

        impl std::str::FromStr for AsciiControl {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                $(
                    if s.eq_ignore_ascii_case(stringify!($name)) {
                        return Ok(AsciiControl::$variant);
                    }
                )*
                Err(())
            }
        }
    };
}

ascii_control! {
    (Nul, 0),
    (Soh, 1),
    (Stx, 2),
    (Etx, 3),
    (Eot, 4),
    (Enq, 5),
    (Ack, 6),
    (Bel, 7),
    (Bs, 8),
    (Tab, 9),
    (Lf, 10),
    (Vt, 11),
    (Ff, 12),
    (Cr, 13),
    (So, 14),
    (Si, 15),
    (Dle, 16),
    (Dc1, 17),
    (Dc2, 18),
    (Dc3, 19),
    (Dc4, 20),
    (Nak, 21),
    (Syn, 22),
    (Etb, 23),
    (Can, 24),
    (Em, 25),
    (Sub, 26),
    (Esc, 27),
    (Fs, 28),
    (Gs, 29),
    (Rs, 30),
    (Us, 31),
    (Del, 127),
}

#[doc(hidden)]
pub fn decode_string(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Collect characters until '>'
            let mut control_name = String::new();
            let mut found_closing = false;
            for ch in chars.by_ref() {
                if ch == '>' {
                    found_closing = true;
                    break;
                }
                control_name.push(ch);
            }

            // Check if it's a hex byte (2 hex digits)
            if control_name.len() == 2
                && control_name.chars().all(|c| c.is_ascii_hexdigit())
                && let Ok(byte) = u8::from_str_radix(&control_name, 16)
            {
                result.push(byte);
                continue;
            }

            // Parse the control name and convert to byte
            match control_name.to_uppercase().as_str() {
                "NUL" => result.push(0),
                "SOH" => result.push(1),
                "STX" => result.push(2),
                "ETX" => result.push(3),
                "EOT" => result.push(4),
                "ENQ" => result.push(5),
                "ACK" => result.push(6),
                "BEL" => result.push(7),
                "BS" => result.push(8),
                "HT" | "TAB" => result.push(9),
                "LF" => result.push(10),
                "VT" => result.push(11),
                "FF" => result.push(12),
                "CR" => result.push(13),
                "SO" => result.push(14),
                "SI" => result.push(15),
                "DLE" => result.push(16),
                "DC1" => result.push(17),
                "DC2" => result.push(18),
                "DC3" => result.push(19),
                "DC4" => result.push(20),
                "NAK" => result.push(21),
                "SYN" => result.push(22),
                "ETB" => result.push(23),
                "CAN" => result.push(24),
                "EM" => result.push(25),
                "SUB" => result.push(26),
                "ESC" => result.push(27),
                "FS" => result.push(28),
                "GS" => result.push(29),
                "RS" => result.push(30),
                "US" => result.push(31),
                // Note that this is only for parsing
                "SP" => result.push(32),
                "DEL" => result.push(127),
                _ => {
                    // If not a recognized control code, treat as literal text
                    result.push(b'<');
                    result.extend_from_slice(control_name.as_bytes());
                    if found_closing {
                        result.push(b'>');
                    }
                }
            }
        } else {
            // Regular character, convert to byte
            let mut buf = [0; 4];
            let char_bytes = ch.encode_utf8(&mut buf);
            result.extend_from_slice(char_bytes.as_bytes());
        }
    }

    result
}

#[doc(hidden)]
pub fn encode_string(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if let Ok(c) = AsciiControl::try_from(c) {
                write!(s, "{}", c).unwrap();
            } else {
                write!(s, "{}", c).unwrap();
            }
        }
        if !chunk.invalid().is_empty() {
            write!(s, "<{}>", hex::encode(chunk.invalid())).unwrap();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_string_unclosed_control_sequence() {
        // Test that unclosed control sequences don't add extraneous '>'
        let decoded = decode_string("<ESC>[<u");
        assert_eq!(decoded, vec![0x1B, 0x5B, 0x3C, 0x75]);
    }

    #[test]
    fn test_decode_string_closed_control_sequence() {
        // Test that closed control sequences work correctly
        let decoded = decode_string("<ESC>[>u");
        assert_eq!(decoded, vec![0x1B, 0x5B, 0x3E, 0x75]);
    }

    #[test]
    fn test_decode_string_unrecognized_closed() {
        // Test unrecognized but closed control sequence
        let decoded = decode_string("<foo>");
        assert_eq!(decoded, vec![0x3C, 0x66, 0x6F, 0x6F, 0x3E]);
    }

    #[test]
    fn test_decode_string_unrecognized_unclosed() {
        // Test unrecognized and unclosed control sequence
        let decoded = decode_string("<bar");
        assert_eq!(decoded, vec![0x3C, 0x62, 0x61, 0x72]);
    }

    #[test]
    fn test_decode_string_recognized_control() {
        // Test recognized control codes
        let decoded = decode_string("<ESC>");
        assert_eq!(decoded, vec![0x1B]);

        let decoded = decode_string("<CR><LF>");
        assert_eq!(decoded, vec![0x0D, 0x0A]);
    }

    #[test]
    fn test_decode_string_hex_bytes() {
        // Test hex byte encoding
        let decoded = decode_string("<1B>[A");
        assert_eq!(decoded, vec![0x1B, 0x5B, 0x41]);
    }
}
