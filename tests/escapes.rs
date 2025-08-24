use pretty_assertions::assert_eq;
use vt_push_parser::ascii::{decode_string, encode_string};
use vt_push_parser::{VTEvent, VTPushParser};

const INPUT: &str = include_str!("escapes.txt");

enum VTAccumulator {
    Raw(String),
    Esc(String),
    Dcs(String),
    Osc(String),
}

fn parse(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new();
    let mut result = Vec::new();
    let mut callback = |vt_input: VTEvent<'_>| match vt_input {
        VTEvent::Raw(s) => {
            if let Some(VTAccumulator::Raw(acc)) = result.last_mut() {
                acc.push_str(&encode_string(s));
            } else {
                result.push(VTAccumulator::Raw(encode_string(s)));
            }
        }
        VTEvent::Csi { .. } | VTEvent::Ss3 { .. } | VTEvent::Esc { .. } | VTEvent::C0(_) => {
            result.push(VTAccumulator::Esc(format!("{vt_input:?}")))
        }
        VTEvent::DcsStart { .. } => result.push(VTAccumulator::Dcs(format!("{vt_input:?}, data="))),
        VTEvent::DcsData(s) => {
            let VTAccumulator::Dcs(acc) = result.last_mut().unwrap() else {
                panic!("DcsData without DcsStart");
            };
            acc.push_str(&encode_string(s));
        }
        VTEvent::DcsEnd => {}
        VTEvent::DcsCancel => {
            let VTAccumulator::Dcs(acc) = result.last_mut().unwrap() else {
                panic!("DcsCancel without DcsStart");
            };
            *acc = format!("{} (cancelled)", acc.split_once(", data=").unwrap().0);
        }
        VTEvent::OscStart => result.push(VTAccumulator::Osc("OscStart, data=".to_string())),
        VTEvent::OscData(s) => {
            let VTAccumulator::Osc(acc) = result.last_mut().unwrap() else {
                panic!("OscData without OscStart");
            };
            acc.push_str(&encode_string(s));
        }
        VTEvent::OscEnd { .. } => {}
        VTEvent::OscCancel => {
            let VTAccumulator::Osc(acc) = result.last_mut().unwrap() else {
                panic!("OscCancel without OscStart");
            };
            *acc = format!("{} (cancelled)", acc.split_once(", data=").unwrap().0);
        }
    };
    for chunk in data {
        parser.feed_with(chunk, &mut callback);
    }

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
    let result = parse(&[decoded]);

    // Ensure that the result is the same when parsing in various chunk sizes
    for chunk_size in 1..=decoded.len() {
        let mut byte_by_byte = Vec::new();
        for b in decoded.chunks(chunk_size) {
            byte_by_byte.push(b);
        }
        let result_byte_by_byte = parse(&byte_by_byte);
        assert_eq!(
            result, result_byte_by_byte,
            "Failed to parse in chunks of size {chunk_size}"
        );
    }

    // Ensure that prefix and suffix to each side of the decoded data are parsed
    // correctly. This should probably be more of a fuzz test instead.
    let result_prefix = parse(&[b"prefix", decoded]);
    let e1 = format!("prefix\n{result}");
    let e2 = format!("prefix{result}");
    if result_prefix != e1 && result_prefix != e2 {
        panic!(
            "Prefix string did not match expectations:\n{result_prefix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
        );
    }

    let result_suffix = parse(&[decoded, b"suffix"]);
    let e1 = format!("{result}suffix\n");
    let e2 = format!("{}suffix\n", result.trim_end());
    if result_suffix != e1 && result_suffix != e2 {
        panic!(
            "Suffix string did not match expectations:\n{result_suffix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
        );
    }

    let result_prefix = parse(&["✅🛜".as_bytes(), decoded]);
    let e1 = format!("✅🛜\n{result}");
    let e2 = format!("✅🛜{result}");
    if result_prefix != e1 && result_prefix != e2 {
        panic!(
            "Prefix string did not match expectations:\n{result_prefix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
        );
    }

    output.push_str(&format!("## {test_name}\n```\n{}\n```\n\n", line));
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
        let test_name_clone = test_name.clone();
        let Ok(test_output) = std::panic::catch_unwind(move || {
            let mut output = String::new();
            test(&mut output, &test_name_clone, line, &decoded);
            output
        }) else {
            eprintln!("  test {:?} panicked", test_name);
            failures += 1;
            continue;
        };
        output.push_str(&test_output);
    }

    println!();

    if failures > 0 {
        eprintln!("{} tests failed", failures);
        std::process::exit(1);
    }

    if std::env::var("UPDATE").is_ok() {
        std::fs::write("tests/result.md", output).unwrap();
    } else {
        let expected = std::fs::read_to_string("tests/result.md").unwrap();
        assert_eq!(expected, output);
        println!("all tests passed");
    }
}
