mod common;

use vt_push_parser::VT_PARSER_INTEREST_INPUT;

const INPUT: &str = include_str!("input_escapes.txt");

fn extra_validation(_test_name: &str, _result: &str, _decoded: &[u8]) {
    // Input mode tests don't need the extra validation that output mode
    // tests do (prefix/suffix tests, re-encoding checks, etc.)
}

pub fn main() {
    common::run_tests::<VT_PARSER_INTEREST_INPUT>(
        common::TestConfig {
            input_file: INPUT,
            output_file: "tests/input_result.md",
            title: "Input Escapes",
        },
        &extra_validation,
    );
}
