use pretty_assertions::assert_eq;
use vt_push_parser::VTPushParser;
use vt_push_parser::ascii::{decode_string, encode_string};
use vt_push_parser::event::VTEvent;

pub enum VTAccumulator {
    Raw(String),
    Esc(String),
    Dcs(String),
    Osc(String),
}

macro_rules! callback {
    ($result:ident, $ret:expr) => {
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
                VTEvent::Csi { .. } | VTEvent::Esc { .. } | VTEvent::C0(_) => {
                    result.push(VTAccumulator::Esc(format!("{vt_input:?}")))
                }
                VTEvent::Ss2 { .. } | VTEvent::Ss3 { .. } => {
                    result.push(VTAccumulator::Esc(format!("{vt_input:?}")))
                }

                VTEvent::DcsStart { .. } => {
                    result.push(VTAccumulator::Dcs(format!("{vt_input:?}, data=")))
                }
                VTEvent::DcsData(s) | VTEvent::DcsEnd(s) => {
                    let VTAccumulator::Dcs(acc) = result.last_mut().unwrap() else {
                        panic!("DcsData without DcsStart");
                    };
                    acc.push_str(&encode_string(s));
                }
                VTEvent::DcsCancel => {
                    let VTAccumulator::Dcs(acc) = result.last_mut().unwrap() else {
                        panic!("DcsCancel without DcsStart");
                    };
                    *acc = format!("{} (cancelled)", acc.split_once(", data=").unwrap().0);
                }
                VTEvent::OscStart => result.push(VTAccumulator::Osc("OscStart, data=".to_string())),
                VTEvent::OscData(s) | VTEvent::OscEnd { data: s, .. } => {
                    let VTAccumulator::Osc(acc) = result.last_mut().unwrap() else {
                        panic!("OscData without OscStart");
                    };
                    acc.push_str(&encode_string(s));
                }
                VTEvent::OscCancel => {
                    let VTAccumulator::Osc(acc) = result.last_mut().unwrap() else {
                        panic!("OscCancel without OscStart");
                    };
                    *acc = format!("{} (cancelled)", acc.split_once(", data=").unwrap().0);
                }
            };
            $ret
        }
    };
}

#[derive(Clone, Copy, Debug)]
pub enum ParseMode {
    Normal,
    Abortable,
    Aborted,
}

impl ParseMode {
    #[allow(dead_code)]
    pub fn all() -> &'static [ParseMode] {
        &[ParseMode::Normal, ParseMode::Abortable, ParseMode::Aborted]
    }
}

pub fn parse<const INTEREST: u8>(mode: ParseMode, data: &[&[u8]]) -> String {
    match mode {
        ParseMode::Normal => parse_normal::<INTEREST>(data),
        ParseMode::Abortable => parse_abortable::<INTEREST>(data),
        ParseMode::Aborted => parse_aborted::<INTEREST>(data),
    }
}

fn parse_normal<const INTEREST: u8>(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new_with_interest::<INTEREST>();
    let mut result = Vec::new();
    let mut callback = callback!(result, ());
    for chunk in data {
        parser.feed_with(chunk, &mut callback);
    }
    collect(result)
}

fn parse_abortable<const INTEREST: u8>(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new_with_interest::<INTEREST>();
    let mut result = Vec::new();
    let mut callback = callback!(result, true);
    for chunk in data {
        assert_eq!(
            parser.feed_with_abortable(chunk, &mut callback),
            chunk.len()
        );
    }
    collect(result)
}

fn parse_aborted<const INTEREST: u8>(data: &[&[u8]]) -> String {
    let mut parser = VTPushParser::new_with_interest::<INTEREST>();
    let mut result = Vec::new();
    let mut callback = callback!(result, false);
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
    collect(result)
}

pub fn collect(result: Vec<VTAccumulator>) -> String {
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

pub struct TestConfig {
    pub input_file: &'static str,
    pub output_file: &'static str,
    pub title: &'static str,
}

pub fn run_tests<const INTEREST: u8>(
    config: TestConfig,
    extra_validation: &(impl Fn(&str, &str, &[u8]) + std::panic::UnwindSafe + std::panic::RefUnwindSafe),
) {
    let mut output = String::new();
    let mut failures = 0;
    output.push_str(&format!("# {}\n", config.title));

    let filter = std::env::args().nth(1).unwrap_or_default();

    let mut test_name = String::new();
    for line in config.input_file.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(stripped_name) = line.trim().strip_prefix("# ") {
            test_name = stripped_name.to_owned();
            continue;
        }

        if !filter.is_empty() && !test_name.contains(&filter) {
            continue;
        }

        let decoded = decode_string(line);
        println!("  running {:?} ...", test_name);
        let test_name_clone = test_name.clone();
        let line_clone = line.to_string();
        let Ok(test_output) = std::panic::catch_unwind(move || {
            let mut output = String::new();
            test::<INTEREST>(
                &mut output,
                &test_name_clone,
                &line_clone,
                &decoded,
                extra_validation,
            );
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

    if filter.is_empty() {
        if std::env::var("UPDATE").is_ok() {
            std::fs::write(config.output_file, output).unwrap();
        } else {
            let expected = std::fs::read_to_string(config.output_file).unwrap();
            assert_eq!(expected, output);
            println!("all tests passed");
        }
    }
}

fn test<const INTEREST: u8>(
    output: &mut String,
    test_name: &str,
    line: &str,
    decoded: &[u8],
    extra_validation: impl Fn(&str, &str, &[u8]),
) {
    let result = parse::<INTEREST>(ParseMode::Normal, &[decoded]);

    // Ensure the result is the same when stepping forward with a
    // cancellable parser
    let result_abortable = parse::<INTEREST>(ParseMode::Abortable, &[decoded]);
    assert_eq!(result, result_abortable);
    let result_aborted = parse::<INTEREST>(ParseMode::Aborted, &[decoded]);
    assert_eq!(
        result, result_aborted,
        "Stepped parser should yield the same results"
    );

    // Ensure that the result is the same when parsing in various chunk sizes
    for chunk_size in 1..=decoded.len() {
        let mut byte_by_byte = Vec::new();
        for b in decoded.chunks(chunk_size) {
            byte_by_byte.push(b);
        }
        let result_byte_by_byte = parse::<INTEREST>(ParseMode::Normal, &byte_by_byte);
        assert_eq!(
            result,
            result_byte_by_byte,
            "Failed to parse in chunks of size {chunk_size} ({:02X?})",
            decoded.chunks(chunk_size).collect::<Vec<_>>()
        );
    }

    // Run any extra validation specific to this test configuration
    extra_validation(test_name, &result, decoded);

    output.push_str(&format!("## {test_name}\n```\n{}\n```\n\n", line));
    output.push_str("```\n");
    output.push_str(&result);
    output.push_str("```\n");
    output.push_str("---\n");
}
