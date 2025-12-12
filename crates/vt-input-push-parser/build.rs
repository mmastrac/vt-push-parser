use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;

use vt_push_parser::VTPushParser;
use vt_push_parser::ascii::AsciiControl;
use vt_push_parser::signature::VTEscapeSignature;

#[derive(Debug, Clone)]
struct Match {
    sequence: String,
    match_type: MatchType,
    key: KeyModifier,
    key_sequence: KeySequence,
}

#[derive(Debug, Clone)]
enum Key {
    Char(char),
    Named(String),
}

#[derive(Debug, Clone)]
enum KeySequence {
    Regular(u16, u8),
    Irregular(Vec<u8>),
}

#[derive(Debug, Clone)]
struct KeyModifier {
    key: Key,
    modifiers: Vec<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum MatchType {
    Nothing,
    Normal,
    Weak,
    WeakEnd,
    Reject,
}

fn parse_key(key: &str) -> (KeyModifier, String) {
    // ','(KEYPAD+VT52): ESC ? l
    // F15(SUN): CSI 196 z

    // First split on colon+space
    let (key, sequence) = key.split_once(": ").unwrap();
    let (key, modifiers) = if key.ends_with(')') {
        // Split at last '('
        let (key, mut modifiers) = key.rsplit_once('(').unwrap();
        modifiers = modifiers.trim_end_matches(')');
        let modifiers = modifiers.split('+').map(|m| m.to_string()).collect();
        (key, modifiers)
    } else {
        (key, vec![])
    };

    if key.starts_with("'") && key.ends_with("'") {
        (
            KeyModifier {
                key: Key::Char(key.chars().nth(1).unwrap()),
                modifiers,
            },
            sequence.to_string(),
        )
    } else {
        (
            KeyModifier {
                key: Key::Named(key.to_string()),
                modifiers,
            },
            sequence.to_string(),
        )
    }
}

fn decode_sequence(sequence: &str) -> Vec<u8> {
    // Split the sequence on space
    let mut sequence_bytes = Vec::new();
    for part in sequence.split(' ') {
        match part {
            "ESC" => sequence_bytes.push(0x1B),
            "CSI" => sequence_bytes.extend_from_slice(b"\x1b["),
            "SS3" => sequence_bytes.extend_from_slice(b"\x1bO"),
            "OSC" => sequence_bytes.extend_from_slice(b"\x1b]"),
            "SP" => sequence_bytes.push(0x20),
            _ => {
                if part.starts_with('<') && part.ends_with('>') {
                    let ascii_control = part.trim_start_matches('<').trim_end_matches('>');
                    let hex = u8::from_str_radix(ascii_control, 16).unwrap();
                    sequence_bytes.push(hex);
                } else if let Ok(ascii_control) = AsciiControl::from_str(part) {
                    sequence_bytes.push(ascii_control as u8);
                } else if part.chars().all(|c| c.is_ascii_digit()) {
                    sequence_bytes.extend_from_slice(part.as_bytes());
                } else if part.len() == 1 {
                    sequence_bytes.extend_from_slice(part.as_bytes());
                } else {
                    panic!("Invalid part: {part:?}");
                }
            }
        }
    }
    sequence_bytes
}

fn generate_matcher_fn(
    fn_name: &str,
    more_data: bool,
    patterns: &[(&[u8], (String, MatchType))],
) -> String {
    let mut code = String::new();

    struct Node<'a> {
        value: &'a str,
        match_type: MatchType,
        children: BTreeMap<u8, Node<'a>>,
    }

    fn insert<'a>(root: &mut Node<'a>, pat: &'a [u8], val: &'a str, match_type: MatchType) {
        let mut node = root;
        for &b in pat {
            // Copy the parent node's value
            node = node
                .children
                .entry(b)
                .or_insert_with(|| match node.match_type {
                    MatchType::WeakEnd => Node {
                        value: "",
                        match_type: MatchType::Weak,
                        children: BTreeMap::new(),
                    },
                    _ => Node {
                        value: node.value,
                        match_type: node.match_type,
                        children: BTreeMap::new(),
                    },
                });
        }
        node.value = val;
        node.match_type = match_type;
    }

    let mut root = Node {
        value: "",
        match_type: MatchType::Nothing,
        children: BTreeMap::new(),
    };
    for (pat, (val, match_type)) in patterns {
        insert(&mut root, pat, val, *match_type);
    }

    // Recursively write the trie as a comment for debugging
    fn write_trie(byte: u8, node: &Node, out: &mut String, indent: usize) {
        let ind = "    ".repeat(indent);
        out.push_str(&format!(
            "{ind}{byte:02x}: {} {:?}\n",
            node.value, node.match_type
        ));
        for (b, child) in &node.children {
            write_trie(*b, child, out, indent + 1);
        }
    }
    let mut trie_str = String::new();
    write_trie(0, &root, &mut trie_str, 0);
    code.push_str(&format!("/* Trie:\n{trie_str}\n*/\n"));
    eprintln!("Trie:\n{trie_str}");

    fn fmt_byte(b: u8) -> String {
        if b.is_ascii_graphic() || b == b' ' {
            format!("b'{}'", b as char)
        } else {
            format!("{b:#04x}")
        }
    }

    fn emit(
        node: &Node,
        value: u8,
        depth: usize,
        out: &mut String,
        indent: usize,
        more_data: bool,
    ) {
        let ind = "    ".repeat(indent);
        let ind2 = "    ".repeat(indent + 1);
        let ind3 = "    ".repeat(indent + 2);

        out.push_str("{\n");

        let val = node.value;
        match (more_data, node.children.is_empty(), node.match_type) {
            // Leaf: unconditional match if we have no children
            (_, true, MatchType::Normal) => {
                out.push_str(&format!("{ind2}return {val}; // leaf match\n"));
                out.push_str(&format!("{ind}}}"));
                return;
            }
            (false, _, MatchType::Normal) => {
                // Otherwise, match if we have the right length
                out.push_str(&format!("{ind2}if s_len == {depth} {{ return {val}; }}\n"));
            }
            (true, _, MatchType::Normal) => {
                // Otherwise, pending match if that's all the data we have
                out.push_str(&format!(
                    "{ind2}if s_len == {depth} {{ return MatchResult::PendingMatch; }} // normal\n"
                ));
            }
            (_, _, MatchType::Nothing) => {}
            (_, no_children, MatchType::Weak) => {
                assert!(!no_children);
            }
            (false, true, MatchType::WeakEnd) => {
                // Weak end only matches if idle AND we're at the end,
                // otherwise it's a reject. Any other time we don't match
                // it.
                out.push_str(&format!("{ind2}if s_len == {depth} {{ return {val}; }}\n"));
                out.push_str(&format!(
                    "{ind2}return MatchResult::NoMatch {{ length: {depth} }};\n"
                ));
                out.push_str(&format!("{ind}}}"));
                return;
            }
            (false, false, MatchType::WeakEnd) => {
                out.push_str(&format!("{ind2}if s_len == {depth} {{ return {val}; }}\n"));

                // fall through to children
                out.push_str(&format!("{ind2}// children after\n"));
            }
            (true, true, MatchType::WeakEnd) => {
                out.push_str(&format!(
                    "{ind2}return MatchResult::NoMatch {{ length: {depth} }};\n"
                ));
                out.push_str(&format!("{ind}}}"));
                return;
            }
            (true, false, MatchType::WeakEnd) => {
                // fall through to children
                out.push_str(&format!("{ind2}// children only\n"));
            }
            (_, no_children, MatchType::Reject) => {
                assert!(no_children);
                out.push_str(&format!(
                    "{ind2}return MatchResult::NoMatch {{ length: {depth} }}; // reject match\n"
                ));
                out.push_str(&format!("{ind}}}"));
                return;
            }
        }

        // Now, we recurse...
        assert!(!node.children.is_empty());

        out.push_str(&format!("{ind2}if s_len > {depth} {{\n"));
        out.push_str(&format!("{ind3}match bytes[{depth}] {{\n"));
        for (b, child) in &node.children {
            out.push_str(&format!("{ind3}    {} => ", fmt_byte(*b)));
            emit(child, *b, depth + 1, out, indent + 3, more_data);
            out.push_str(",\n");
        }

        if depth == 1 && value == 0x1b {
            out.push_str("                    0x20..=0x7e => utf8_1(true, bytes[1]),\n");
            out.push_str("                    0xc2..=0xdf => utf8_2(true, &bytes[1..]),\n");
            out.push_str("                    0xe0..=0xef => utf8_3(true, &bytes[1..]),\n");
            out.push_str("                    0xf0..=0xf4 => utf8_4(true, &bytes[1..]),\n");
        }

        match (more_data, node.match_type) {
            (true, MatchType::Weak) => {
                out.push_str(&format!(
                    "{ind3}    _ => return MatchResult::PendingMatch,\n"
                ));
            }
            (false, MatchType::Weak) => {
                out.push_str(&format!(
                    "{ind3}    _ => return MatchResult::NoMatch {{ length: {depth} }},\n"
                ));
            }
            (_, MatchType::WeakEnd) => {
                // out.push_str(&format!("{ind3}    _ => return {}, // fallback\n", node.value));
                out.push_str(&format!(
                    "{ind3}    _ => return MatchResult::NoMatch {{ length: {depth} }},\n"
                ));
            }
            (_, MatchType::Nothing) => {
                out.push_str(&format!(
                    "{ind3}    _ => return MatchResult::NoMatch {{ length: {depth} }},\n"
                ));
            }
            (_, _) => {
                // Default: fall back
                out.push_str(&format!(
                    "{ind3}    _ => return {}, // fallback\n",
                    node.value
                ));
            }
        }

        out.push_str(&format!("{ind3}}}\n")); // end match
        match (more_data, node.match_type) {
            (true, MatchType::Weak) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::PendingMatch; }} // child\n"
                ));
            }
            (false, MatchType::Weak) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::NoMatch {{ length: {depth} }}; }}\n"
                ));
            }
            (true, MatchType::WeakEnd) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::PendingMatch; }}\n"
                ));
            }
            (false, MatchType::WeakEnd) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::NoMatch {{ length: {depth} }}; }}\n"
                ));
            }
            (true, MatchType::Nothing) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::PendingMatch; }}\n"
                ));
            }
            (false, MatchType::Nothing) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::NoMatch {{ length: {depth} }}; }}\n"
                ));
            }
            (true, _) => {
                out.push_str(&format!(
                    "{ind2}}} else {{ return MatchResult::PendingMatch; }} // child\n"
                ));
            }
            (false, _) => {
                out.push_str(&format!("{ind2}}} else {{ return {}; }}\n", node.value));
            }
        }

        out.push_str(&format!("{ind}}}"));
    }

    // Root
    code.push_str(&format!(
        "pub fn {fn_name}(bytes: &[u8]) -> MatchResult {{\n"
    ));
    code.push_str("    let s_len = bytes.len();\n");
    code.push_str("    if s_len == 0 { return MatchResult::PendingMatch; }\n");

    code.push_str("    match bytes[0] {\n");

    for (b, child) in &root.children {
        code.push_str(&format!("        {} => ", fmt_byte(*b)));
        emit(child, *b, 1, &mut code, 2, more_data);
        code.push_str(",\n");
    }

    code.push_str("        0x20..=0x7e => utf8_1(false, bytes[0]),\n");
    code.push_str("        0xc2..=0xdf => utf8_2(false, &bytes),\n");
    code.push_str("        0xe0..=0xef => utf8_3(false, &bytes),\n");
    code.push_str("        0xf0..=0xf4 => utf8_4(false, &bytes),\n");
    code.push_str("        0x80..=0xc1 | 0xf5..=0xff => invalid_utf8(bytes[0]),\n");
    code.push_str("        _ => MatchResult::NoMatch { length: 1 },\n");
    code.push_str("    }\n");
    code.push_str("}\n");
    code
}

fn main() {
    let keys = include_str!("src/keys.txt");
    let output_file = Path::new(&std::env::var("OUT_DIR").unwrap()).join("keys.rs");
    let mut key_sequence_file = File::create(output_file).unwrap();
    let mut all_keys = Vec::new();
    let mut key_codes_u = BTreeMap::new();
    let mut key_codes_tilde = BTreeMap::new();
    let mut key_codes_sun = BTreeMap::new();
    let mut all_sequence_set = BTreeSet::new();

    for (i, mut line) in keys.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with("//") {
            continue;
        }
        if line.contains(" // ") {
            line = line.split(" // ").next().unwrap();
        }
        let (line, match_type) = if let Some(line) = line.strip_prefix("? ") {
            (line, MatchType::Weak)
        } else if let Some(line) = line.strip_prefix("?? ") {
            (line, MatchType::WeakEnd)
        } else {
            (line, MatchType::Normal)
        };

        // For easier build.rs debugging
        eprintln!("line {i}: {line}");

        let (key, sequence, match_type) = if let Some(line) = line.strip_prefix("!: ") {
            (
                KeyModifier {
                    key: Key::Char(char::MIN),
                    modifiers: vec![],
                },
                decode_sequence(line),
                MatchType::Reject,
            )
        } else {
            let (key, sequence) = parse_key(line);
            (key, decode_sequence(&sequence), match_type)
        };
        let mut key_sequence = KeySequence::Irregular(sequence.clone());
        if !all_sequence_set.insert(sequence.clone()) {
            panic!("Duplicate sequence: {sequence:?}");
        }
        VTPushParser::decode_buffer(&sequence, |event| {
            const CSI_KEY_U: VTEscapeSignature = VTEscapeSignature::csi(b'u').with_params_exact(1);
            const CSI_KEY_TILDE: VTEscapeSignature =
                VTEscapeSignature::csi(b'~').with_params_exact(1);
            const CSI_KEY_SUN: VTEscapeSignature =
                VTEscapeSignature::csi(b'z').with_params_exact(1);

            if CSI_KEY_U.matches(&event) {
                let csi = event.csi().unwrap();
                let param = csi.params.try_parse::<u16>(0).unwrap();
                if let Key::Named(key) = key.key.clone() {
                    key_codes_u.insert(key, param);
                }
                key_sequence = KeySequence::Regular(param, b'u');
            }
            if CSI_KEY_TILDE.matches(&event) {
                let csi = event.csi().unwrap();
                let param = csi.params.try_parse::<u16>(0).unwrap();
                if let Key::Named(key) = key.key.clone() {
                    key_codes_tilde.insert(key, param);
                }
                key_sequence = KeySequence::Regular(param, b'~');
            }
            if CSI_KEY_SUN.matches(&event) {
                let csi = event.csi().unwrap();
                let param = csi.params.try_parse::<u16>(0).unwrap();
                if let Key::Named(key) = key.key.clone() {
                    key_codes_sun.insert(key, param);
                }
                key_sequence = KeySequence::Regular(param, b'z');
            }
        });

        let sequence = String::from_utf8(sequence).unwrap();
        let m = Match {
            sequence,
            match_type,
            key,
            key_sequence,
        };
        writeln!(key_sequence_file, "// {m:?}").unwrap();

        all_keys.push(m);
    }

    // Collect all modifiers and named keys
    let mut modifiers = BTreeSet::new();
    let mut named_keys = BTreeSet::new();
    let mut all_sequences = BTreeMap::new();
    for m in all_keys {
        modifiers.extend(m.key.modifiers.clone());
        if let Key::Named(key) = &m.key.key {
            named_keys.insert(key.clone());
        }
        let KeySequence::Irregular(irregular) = m.key_sequence else {
            continue;
        };
        let len = irregular.len();
        let payload = match m.key.key {
            Key::Char(c) => {
                format!("e!({len}, {c:?}, ({}))", m.key.modifiers.join(", "))
            }
            Key::Named(n) => {
                format!("e!({len}, {n}, ({}))", m.key.modifiers.join(", "))
            }
        };

        if all_sequences
            .insert(m.sequence.clone(), (payload, m.match_type))
            .is_some()
        {
            panic!("Duplicate sequence: {:?}", m.sequence);
        }
    }

    writeln!(
        key_sequence_file,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::TryFrom)]"
    )
    .unwrap();
    writeln!(key_sequence_file, "#[try_from(repr)]").unwrap();
    writeln!(key_sequence_file, "#[repr(u8)]").unwrap();
    writeln!(key_sequence_file, "pub enum Key {{").unwrap();
    for key in named_keys {
        writeln!(key_sequence_file, "    {key},").unwrap();
    }
    writeln!(key_sequence_file, "}}").unwrap();

    let all_keys = key_codes_u
        .keys()
        .chain(key_codes_tilde.keys())
        .collect::<BTreeSet<_>>();
    writeln!(
        key_sequence_file,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(key_sequence_file, "pub enum KeyCode {{").unwrap();
    for key in all_keys {
        writeln!(key_sequence_file, "    {key},").unwrap();
    }
    writeln!(key_sequence_file, "}}").unwrap();

    writeln!(key_sequence_file, "enum KeyCodeU {{").unwrap();
    for (key, code) in key_codes_u {
        writeln!(key_sequence_file, "    {key} = {code},").unwrap();
    }
    writeln!(key_sequence_file, "}}").unwrap();

    writeln!(key_sequence_file, "enum KeyCodeTilde {{").unwrap();
    for (key, code) in key_codes_tilde {
        writeln!(key_sequence_file, "    {key} = {code},").unwrap();
    }
    writeln!(key_sequence_file, "}}").unwrap();

    let max_sequence_len = all_sequences.keys().map(|s| s.len()).max().unwrap();
    writeln!(
        key_sequence_file,
        "pub const MAX_SEQUENCE_LEN: usize = {max_sequence_len};"
    )
    .unwrap();

    let all_sequences = all_sequences
        .iter()
        .map(|s| (s.0.as_bytes(), s.1.clone()))
        .collect::<Vec<_>>();
    let matcher_fn = generate_matcher_fn("find_sequence_nonidle_gen", true, &all_sequences);
    write!(key_sequence_file, "{matcher_fn}").unwrap();
    let matcher_fn = generate_matcher_fn("find_sequence_idle_gen", false, &all_sequences);
    write!(key_sequence_file, "{matcher_fn}").unwrap();

    // Check for missing disambiguation sequences (ie: `ESC x ..` should have `ESC x` present)
    let mut missing_sequences = BTreeSet::new();
    for sequence in &all_sequence_set {
        if let &[0x1b, x, ..] = sequence.as_slice() {
            let candidate: &[u8] = &[0x1b, x];
            if !all_sequence_set.contains(candidate) {
                missing_sequences.insert(format!("ESC <{x:02x}> ({:?})", x as char));
            }
        }
    }

    // if !missing_sequences.is_empty() {
    //     panic!(
    //         "Missing disambiguation sequences:\n\n{}",
    //         missing_sequences.into_iter().collect::<Vec<_>>().join("\n")
    //     );
    // }
}
