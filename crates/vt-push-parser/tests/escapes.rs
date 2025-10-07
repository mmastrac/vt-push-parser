use pretty_assertions::assert_eq;
use vt_push_parser::ascii::{decode_string, encode_string};
use vt_push_parser::event::VTEvent;
use vt_push_parser::{VT_PARSER_INTEREST_NONE, VTPushParser};

const INPUT: &str = include_str!("escapes.txt");

enum VTAccumulator {
    Raw(String),
    Esc(String),
    Dcs(String),
    Osc(String),
}

macro_rules! callback {
    ($result:ident, $counts:expr, $ret:expr) => {
        |vt_input: VTEvent<'_>| {
            let result = &mut $result;
            match vt_input {
                VTEvent::Raw(s) => {
                    if let Some(VTAccumulator::Raw(acc)) = result.last_mut() {
                        acc.push_str(&encode_string(s));
                    } else {
                        result.push(VTAccumulator::Raw(encode_string(s)));
                    }
                }
                VTEvent::EscInvalid(esc_invalid) => {
                    result.push(VTAccumulator::Esc(format!("{esc_invalid:?}")))
                }
                VTEvent::Csi { .. } | VTEvent::Esc { .. } | VTEvent::C0(_) => {
                    result.push(VTAccumulator::Esc(format!("{vt_input:?}")))
                }
                VTEvent::Ss2 { .. } | VTEvent::Ss3 { .. } => {
                    result.push(VTAccumulator::Esc(format!("{vt_input:?}")))
                }
                VTEvent::DcsStart { .. } => {
                    result.push(VTAccumulator::Dcs(format!("{vt_input:?}, data=")));
                    $counts.0 += 1;
                }
                VTEvent::DcsData(s) | VTEvent::DcsEnd(s) => {
                    let VTAccumulator::Dcs(acc) = result.last_mut().unwrap() else {
                        panic!("DcsData without DcsStart");
                    };
                    acc.push_str(&encode_string(s));
                    if matches!(vt_input, VTEvent::DcsEnd(s)) {
                        $counts.0 -= 1;
                    }
                }
                VTEvent::DcsCancel => {
                    let VTAccumulator::Dcs(acc) = result.last_mut().unwrap() else {
                        panic!("DcsCancel without DcsStart");
                    };
                    *acc = format!("{} (cancelled)", acc.split_once(", data=").unwrap().0);
                    $counts.0 -= 1;
                }
                VTEvent::OscStart => {
                    result.push(VTAccumulator::Osc("OscStart, data=".to_string()));
                    $counts.1 += 1;
                }
                VTEvent::OscData(s) | VTEvent::OscEnd { data: s, .. } => {
                    let VTAccumulator::Osc(acc) = result.last_mut().unwrap() else {
                        panic!("OscData without OscStart");
                    };
                    acc.push_str(&encode_string(s));
                    if matches!(vt_input, VTEvent::OscEnd { data: s, .. }) {
                        $counts.1 -= 1;
                    }
                }
                VTEvent::OscCancel => {
                    let VTAccumulator::Osc(acc) = result.last_mut().unwrap() else {
                        panic!("OscCancel without OscStart");
                    };
                    *acc = format!("{} (cancelled)", acc.split_once(", data=").unwrap().0);
                    $counts.1 -= 1;
                }
            };
            $ret
        }
    };
}

#[derive(Clone, Copy, Debug)]
enum ParseMode {
    Normal,
    Abortable,
    Aborted,
}

impl ParseMode {
    pub fn all() -> &'static [ParseMode] {
        &[ParseMode::Normal, ParseMode::Abortable, ParseMode::Aborted]
    }
}

fn parse(mode: ParseMode, data: &[&[u8]]) -> String {
    match mode {
        ParseMode::Normal => parse_normal(data),
        ParseMode::Abortable => parse_abortable(data),
        ParseMode::Aborted => parse_aborted(data),
    }
}

fn parse_normal(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new();
    let mut result = Vec::new();
    let mut counts = (0, 0);
    let mut callback = callback!(result, counts, ());
    for chunk in data {
        parser.feed_with(chunk, &mut callback);
    }
    assert_eq!(counts, (0, 0), "Start/end counts mismatch");
    collect(result)
}

fn parse_abortable(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new();
    let mut result = Vec::new();
    let mut counts = (0, 0);
    let mut callback = callback!(result, counts, true);
    for chunk in data {
        assert_eq!(
            parser.feed_with_abortable(chunk, &mut callback),
            chunk.len()
        );
    }
    assert_eq!(counts, (0, 0), "Start/end counts mismatch");
    collect(result)
}

fn parse_aborted(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new();
    let mut result = Vec::new();
    let mut counts = (0, 0);
    let mut callback = callback!(result, counts, false);
    for chunk in data {
        let mut chunk = *chunk;
        while !chunk.is_empty() {
            let parsed = parser.feed_with_abortable(chunk, &mut callback);
            assert!(
                parsed <= chunk.len(),
                "Invalid return value for {chunk:?}: {parsed} should be <= {}",
                chunk.len()
            );
            chunk = &chunk[parsed..];
        }
    }
    assert_eq!(counts, (0, 0), "Start/end counts mismatch");
    collect(result)
}

fn collect(result: Vec<VTAccumulator>) -> String {
    let mut result_string = String::new();
    for acc in result {
        match acc {
            VTAccumulator::Raw(s) => result_string.push_str(&s),
            VTAccumulator::Esc(s) => result_string.push_str(&s),
            VTAccumulator::Dcs(s) => result_string.push_str(&s),
            VTAccumulator::Osc(s) => result_string.push_str(&s),
        }
        result_string.push('\n');
    }
    result_string
}

fn test(output: &mut String, test_name: &str, line: &str, decoded: &[u8]) {
    let result = parse(ParseMode::Normal, &[decoded]);

    // Ensure the result is the same when stepping forward with a cancellable parser
    let result_abortable = parse(ParseMode::Abortable, &[decoded]);
    assert_eq!(result, result_abortable);
    let result_aborted = parse(ParseMode::Aborted, &[decoded]);
    assert_eq!(
        result, result_aborted,
        "Stepped parser should yield the same results"
    );

    // Ensure that the result is the same no matter what interest flags are set
    let mut text_content = String::new();
    let mut text_test = b"text content test:<".to_vec();
    text_test.extend_from_slice(decoded);
    text_test.extend_from_slice(b">suffix text context");
    VTPushParser::decode_buffer(&text_test, |event| {
        if let VTEvent::Raw(s) = event {
            text_content.push_str(String::from_utf8_lossy(s).as_ref())
        }
    });

    let mut text_content_interest_none = String::new();
    let mut parser = VTPushParser::new_with_interest::<{ VT_PARSER_INTEREST_NONE }>();
    parser.feed_with(&text_test, &mut |event: VTEvent| {
        if let VTEvent::Raw(s) = event {
            text_content_interest_none.push_str(String::from_utf8_lossy(s).as_ref())
        }
    });
    assert_eq!(text_content, text_content_interest_none);

    for mode in ParseMode::all().iter().cloned() {
        // Ensure that the result is the same when parsing in various chunk sizes
        for chunk_size in 1..=decoded.len() {
            let mut byte_by_byte = Vec::new();
            for b in decoded.chunks(chunk_size) {
                byte_by_byte.push(b);
            }
            let result_byte_by_byte = parse(mode, &byte_by_byte);
            assert_eq!(
                result,
                result_byte_by_byte,
                "Failed to parse in chunks of size {chunk_size} ({:02X?})",
                decoded.chunks(chunk_size).collect::<Vec<_>>()
            );
        }

        // Ensure that prefix and suffix to each side of the decoded data are parsed
        // correctly. This should probably be more of a fuzz test instead.
        let result_prefix = parse(mode, &[b"prefix", decoded]);
        let e1 = format!("prefix\n{result}");
        let e2 = format!("prefix{result}");
        if result_prefix != e1 && result_prefix != e2 {
            panic!(
                "Prefix string did not match expectations:\n{result_prefix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
            );
        }

        let result_suffix = parse(mode, &[decoded, b"suffix"]);
        let e1 = format!("{result}suffix\n");
        let e2 = format!("{}suffix\n", result.trim_end());
        if result_suffix != e1 && result_suffix != e2 {
            panic!(
                "Suffix string did not match expectations:\n{result_suffix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
            );
        }

        let result_prefix = parse(mode, &["✅🛜".as_bytes(), decoded]);
        let e1 = format!("✅🛜\n{result}");
        let e2 = format!("✅🛜{result}");
        if result_prefix != e1 && result_prefix != e2 {
            panic!(
                "Prefix string did not match expectations:\n{result_prefix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
            );
        }
    }

    // Ensure that the re-encoded result is the same as the original
    if !test_name.contains("cancelled")
        && !test_name.contains("invalid")
        && !test_name.starts_with("APC:")
        && !test_name.starts_with("PM:")
        && !test_name.starts_with("SOS:")
    {
        let mut re_encoded = Vec::new();
        let mut raw_del = false;
        VTPushParser::decode_buffer(decoded, |event| {
            let mut buffer = [0_u8; 1024];
            let n = event.encode(&mut buffer).unwrap();
            re_encoded.extend_from_slice(&buffer[..n]);

            if matches!(event, VTEvent::C0(0x7f)) {
                raw_del = true;
            }
        });
        let decoded = if raw_del {
            decoded.to_vec()
        } else {
            decoded
                .iter()
                .cloned()
                .filter(|b| *b != 0x7F)
                .collect::<Vec<_>>()
        };
        if re_encoded != decoded {
            panic!(
                "Re-encoded result did not match original:\n{re_encoded:?}\nExpected:\n{decoded:?}"
            );
        }
    }

    output.push_str(&format!("## {test_name}\n```\n{line}\n```\n\n"));
    output.push_str("```\n");
    output.push_str(&result);
    output.push_str("```\n");
    output.push_str("---\n");
}

pub fn main() {
    println!();
    eprintln!("Testing escapes.txt");

    let mut output = String::new();
    let mut failures = 0;
    output.push_str("# Escapes\n");

    let filter = std::env::args().nth(1).unwrap_or_default();

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

        if !filter.is_empty() && !test_name.contains(&filter) {
            continue;
        }

        let decoded = decode_string(line);
        println!("  running {test_name:?} ...");
        let test_name_clone = test_name.clone();
        let Ok(test_output) = std::panic::catch_unwind(move || {
            let mut output = String::new();
            test(&mut output, &test_name_clone, line, &decoded);
            output
        }) else {
            eprintln!("  test {test_name:?} panicked");
            failures += 1;
            continue;
        };
        output.push_str(&test_output);
    }

    println!();

    if failures > 0 {
        eprintln!("{failures} tests failed");
        std::process::exit(1);
    }

    if filter.is_empty() {
        if std::env::var("UPDATE").is_ok() {
            std::fs::write("tests/result.md", output).unwrap();
        } else {
            let expected = std::fs::read_to_string("tests/result.md").unwrap();
            assert_eq!(expected, output);
            println!("all tests passed");
        }
    }
}
