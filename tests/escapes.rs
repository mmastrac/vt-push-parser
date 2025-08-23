use pretty_assertions::assert_eq;
use vt_push_parser::{VTEvent, VTPushParser};

const INPUT: &str = include_str!("escapes.txt");

fn decode_string(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut chars = input.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Collect characters until '>'
            let mut control_name = String::new();
            while let Some(ch) = chars.next() {
                if ch == '>' {
                    break;
                }
                control_name.push(ch);
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
                "TAB" => result.push(9),
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
                "DEL" => result.push(127),
                _ => {
                    // If not a recognized control code, treat as literal text
                    result.push(b'<');
                    result.extend_from_slice(control_name.as_bytes());
                    result.push(b'>');
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

fn parse(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new();
    let mut result = String::new();
    let mut callback = |vt_input: VTEvent<'_>| {
        result.push_str(&format!("{:?}\n", vt_input));
    };
    for chunk in data {
        parser.feed_with(chunk, &mut callback);
    }
    result
}

fn test(output: &mut String, test_name: &str, line: &str, decoded: &[u8]) {
    let result = parse(&[decoded]);

    // Ensure that prefix and suffix to each side of the decoded data are parsed
    // correctly. This should probably be more of a fuzz test instead.
    let result_prefix = parse(&[b"prefix", decoded]);
    assert_eq!(result_prefix, "Raw('prefix')\n".to_owned() + &result);

    let result_suffix = parse(&[decoded, b"suffix"]);
    assert_eq!(result_suffix, result.clone() + "Raw('suffix')\n");

    let result_prefix = parse(&["✅🛜".as_bytes(), decoded]);
    assert_eq!(result_prefix, "Raw('✅🛜')\n".to_owned() + &result);

    output.push_str(&format!("## {test_name}\n```\n{}\n```\n\n", line));
    output.push_str("```\n");
    output.push_str(&result);
    output.push_str("```\n");
    output.push_str("---\n");
}

pub fn main() {
    eprintln!("Testing escapes.txt");

    let mut output = String::new();
    output.push_str("# Escapes\n");

    let mut test_name = String::new();
    for line in INPUT.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(stripped_name) = line.strip_prefix("# ") {
            test_name = stripped_name.to_owned();
            continue;
        }
        let decoded = decode_string(line);
        println!("  running {:?} ...", test_name);
        test(&mut output, &std::mem::take(&mut test_name), line, &decoded);
    }

    if std::env::var("UPDATE").is_ok() {
        std::fs::write("tests/result.md", output).unwrap();
    } else {
        let expected = std::fs::read_to_string("tests/result.md").unwrap();
        assert_eq!(output, expected);
    }
}
