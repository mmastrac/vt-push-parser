mod common;

use vt_push_parser::event::VTEvent;
use vt_push_parser::{VT_PARSER_INTEREST_NONE, VT_PARSER_INTEREST_OUTPUT, VTPushParser};

const INPUT: &str = include_str!("escapes.txt");

fn extra_validation(test_name: &str, result: &str, decoded: &[u8]) {
    // Ensure that the result is the same no matter what interest flags are set
    let mut text_content = String::new();
    let mut text_test = b"text content test:<".to_vec();
    text_test.extend_from_slice(decoded);
    text_test.extend_from_slice(b">suffix text context");
    VTPushParser::decode_buffer(&text_test, |event| {
        if let VTEvent::Raw(s) = event {
            text_content.push_str(String::from_utf8_lossy(s).as_ref());
        }
    });

    let mut text_content_interest_none = String::new();
    let mut parser = VTPushParser::new_with_interest::<{ VT_PARSER_INTEREST_NONE }>();
    parser.feed_with(&text_test, &mut |event| {
        if let VTEvent::Raw(s) = event {
            text_content_interest_none.push_str(String::from_utf8_lossy(s).as_ref());
        }
    });
    assert_eq!(text_content, text_content_interest_none);

    for mode in common::ParseMode::all().iter().cloned() {
        // Ensure that prefix and suffix to each side of the decoded data are
        // parsed correctly. This should probably be more of a fuzz test
        // instead.
        let result_prefix = common::parse::<VT_PARSER_INTEREST_OUTPUT>(mode, &[b"prefix", decoded]);
        let e1 = format!("prefix\n{result}");
        let e2 = format!("prefix{result}");
        if result_prefix != e1 && result_prefix != e2 {
            panic!(
                "Prefix string did not match expectations:\n{result_prefix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
            );
        }

        let result_suffix = common::parse::<VT_PARSER_INTEREST_OUTPUT>(mode, &[decoded, b"suffix"]);
        let e1 = format!("{result}suffix\n");
        let e2 = format!("{}suffix\n", result.trim_end());
        if result_suffix != e1 && result_suffix != e2 {
            panic!(
                "Suffix string did not match expectations:\n{result_suffix}\nExpected one of:\n{e1}\n-- or --\n\n{e2}"
            );
        }

        let result_prefix =
            common::parse::<VT_PARSER_INTEREST_OUTPUT>(mode, &["✅🛜".as_bytes(), decoded]);
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
}

pub fn main() {
    common::run_tests::<VT_PARSER_INTEREST_OUTPUT>(
        common::TestConfig {
            input_file: INPUT,
            output_file: "tests/result.md",
            title: "Escapes",
        },
        &extra_validation,
    );
}
