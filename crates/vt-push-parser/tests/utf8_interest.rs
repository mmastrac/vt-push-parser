use vt_push_parser::ascii::encode_string;
use vt_push_parser::{
    VT_PARSER_INTEREST_CSI, VT_PARSER_INTEREST_DCS, VT_PARSER_INTEREST_OTHER,
    VT_PARSER_INTEREST_UTF8, VTPushParser, event::VTEvent,
};

#[derive(Clone, Debug, PartialEq)]
enum TestEvent {
    Raw(String),
    Utf8(String),
    Other(String),
}

fn collect_events<const INTEREST: u8>(
    parser: &mut VTPushParser<INTEREST>,
    input: &[u8],
) -> Vec<TestEvent> {
    let mut result = Vec::new();
    parser.feed_with(input, &mut |e: VTEvent<'_>| match e {
        VTEvent::Raw(s) => result.push(TestEvent::Raw(encode_string(s))),
        VTEvent::Utf8(s) => result.push(TestEvent::Utf8(encode_string(s))),
        _ => result.push(TestEvent::Other(format!("{:?}", e))),
    });
    result
}

fn collect_events_multiple<const INTEREST: u8>(
    parser: &mut VTPushParser<INTEREST>,
    inputs: &[&[u8]],
) -> Vec<TestEvent> {
    let mut result = Vec::new();
    for input in inputs {
        parser.feed_with(input, &mut |e: VTEvent<'_>| match e {
            VTEvent::Raw(s) => result.push(TestEvent::Raw(encode_string(s))),
            VTEvent::Utf8(s) => result.push(TestEvent::Utf8(encode_string(s))),
            _ => result.push(TestEvent::Other(format!("{:?}", e))),
        });
    }
    result
}

/// A feed that gives up after `cap` events instead of matching on the parser's
/// own agreement to stop. Used where the input is deliberately malformed: a
/// parser that loops forever on such input should fail as an assertion below,
/// not as a hung test run.
fn capped(
    parser: &mut VTPushParser<
        { VT_PARSER_INTEREST_UTF8 | VT_PARSER_INTEREST_CSI | VT_PARSER_INTEREST_OTHER },
    >,
    input: &[u8],
    cap: usize,
    out: &mut Vec<TestEvent>,
) {
    let mut seen = 0;
    parser.feed_with_abortable(input, &mut |e: VTEvent<'_>| {
        seen += 1;
        match e {
            VTEvent::Raw(s) => out.push(TestEvent::Raw(encode_string(s))),
            VTEvent::Utf8(s) => out.push(TestEvent::Utf8(encode_string(s))),
            _ => out.push(TestEvent::Other(format!("{:?}", e))),
        }
        seen < cap
    });
    assert!(
        seen < cap,
        "parser kept emitting past {cap} events on {input:02x?}: it never finished"
    );
}

#[test]
fn test_utf8_complete_sequence_single_feed() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, "Hello 世界".as_bytes());

    // Should get: Raw("Hello "), Utf8("世"), Utf8("界") - each char is a separate UTF-8 sequence
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], TestEvent::Raw("Hello ".to_string()));
    assert_eq!(events[1], TestEvent::Utf8("世".to_string()));
    assert_eq!(events[2], TestEvent::Utf8("界".to_string()));
}

#[test]
fn test_utf8_partial_split_across_feeds() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events_multiple(&mut parser, &[&[0xe4, 0xb8], &[0x96]]);

    // Should get: Utf8("世") (3 bytes)
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], TestEvent::Utf8("世".to_string()));
}

#[test]
fn test_utf8_mixed_ascii_and_utf8() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events_multiple(
        &mut parser,
        &["ASCII ".as_bytes(), "日本語".as_bytes(), " END".as_bytes()],
    );

    // Should get: Raw("ASCII "), Utf8("日"), Utf8("本"), Utf8("語"), Raw(" END")
    // Each UTF-8 character is emitted as a separate event
    assert_eq!(events.len(), 5);
    assert_eq!(events[0], TestEvent::Raw("ASCII ".to_string()));
    assert_eq!(events[1], TestEvent::Utf8("日".to_string()));
    assert_eq!(events[2], TestEvent::Utf8("本".to_string()));
    assert_eq!(events[3], TestEvent::Utf8("語".to_string()));
    assert_eq!(events[4], TestEvent::Raw(" END".to_string()));
}

#[test]
fn test_utf8_with_escape_sequence() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, "Text \x1b[31mRed\x1b[0m 世界".as_bytes());

    // Should start with Raw("Text ") and end with Utf8("世"), Utf8("界")
    assert!(!events.is_empty());
    assert_eq!(events[0], TestEvent::Raw("Text ".to_string()));
    // The last two events should be the two UTF-8 characters
    assert_eq!(events[events.len() - 2], TestEvent::Utf8("世".to_string()));
    assert_eq!(events[events.len() - 1], TestEvent::Utf8("界".to_string()));
}

#[test]
fn test_utf8_stray_continuation_bytes() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, &[0x80, 0x81, 0x82]);

    // These should be emitted as separate Raw events since they're invalid as standalone
    // encode_string formats invalid bytes as <XX>
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], TestEvent::Raw("<80>".to_string()));
    assert_eq!(events[1], TestEvent::Raw("<81>".to_string()));
    assert_eq!(events[2], TestEvent::Raw("<82>".to_string()));
}

#[test]
fn test_utf8_invalid_continuation() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, &[0xc2, 0x41]); // 0xc2 expects continuation, gets 'A'

    // Lead byte 0xc2 followed by invalid continuation 'A' (0x41)
    // Should emit the lead byte as Raw, then 'A' as Raw
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], TestEvent::Raw("<c2>".to_string()));
    assert_eq!(events[1], TestEvent::Raw("A".to_string()));
}

#[test]
fn test_utf8_four_byte_sequence() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, &[0xf0, 0x9f, 0x98, 0x80]);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0], TestEvent::Utf8("😀".to_string()));
}

#[test]
fn test_utf8_four_byte_split_across_feeds() {
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events_multiple(&mut parser, &[&[0xf0, 0x9f], &[0x98], &[0x80]]);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0], TestEvent::Utf8("😀".to_string()));
}

#[test]
fn test_utf8_without_interest_flag() {
    let mut parser = VTPushParser::new(); // Default interest, no UTF8
    let mut result = Vec::new();
    parser.feed_with("Hello 世界".as_bytes(), &mut |e: VTEvent<'_>| match e {
        VTEvent::Raw(s) => result.push(TestEvent::Raw(encode_string(s))),
        _ => result.push(TestEvent::Other(format!("{:?}", e))),
    });

    // Should get single Raw event with everything
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], TestEvent::Raw("Hello 世界".to_string()));
}

#[test]
fn test_utf8_partial_then_valid_continuation() {
    // Test: feed("AB\xe4") then feed("\xb8\x96") - partial lead then valid continuation
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events_multiple(&mut parser, &[b"AB\xe4", b"\xb8\x96"]);

    // Should get: Raw("AB"), Utf8("世") (U+4E16 = E4 B8 96)
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], TestEvent::Raw("AB".to_string()));
    assert_eq!(events[1], TestEvent::Utf8("世".to_string()));
}

#[test]
fn test_utf8_partial_at_escape() {
    // feed("hi" + e4 b8 + ESC + "[A"): the truncated sequence is flushed as Raw
    // before the escape is parsed, matching the non-UTF-8 path.
    const INTEREST: u8 = VT_PARSER_INTEREST_UTF8 | VT_PARSER_INTEREST_CSI;
    let mut parser = VTPushParser::new_with_interest::<INTEREST>();
    let events = collect_events(&mut parser, b"hi\xe4\xb8\x1b[A");

    assert_eq!(events.len(), 3);
    assert_eq!(events[0], TestEvent::Raw("hi".to_string()));
    assert_eq!(events[1], TestEvent::Raw("<e4b8>".to_string()));
    assert_eq!(events[2], TestEvent::Other("Csi('', 'A')".to_string()));
}

#[test]
fn test_utf8_partial_at_escape_does_not_lose_bytes() {
    // Only the split into events may differ from a parser without the flag: the
    // bytes carried by the Raw events must be the same ones.
    const INTEREST: u8 = VT_PARSER_INTEREST_UTF8 | VT_PARSER_INTEREST_CSI;
    let mut utf8 = VTPushParser::new_with_interest::<INTEREST>();
    let utf8_events = collect_events(&mut utf8, b"hi\xe4\xb8\x1b[A");

    let mut plain = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_CSI>();
    let plain_events = collect_events(&mut plain, b"hi\xe4\xb8\x1b[A");

    let join = |events: &[TestEvent]| {
        events
            .iter()
            .filter_map(|e| match e {
                TestEvent::Raw(s) | TestEvent::Utf8(s) => Some(s.clone()),
                _ => None,
            })
            .collect::<String>()
    };
    assert_eq!(join(&utf8_events), join(&plain_events));
    assert_eq!(utf8_events.last(), plain_events.last());
}

#[test]
fn test_utf8_partial_at_dcs_does_not_lose_bytes() {
    // The same, for a DCS string: the partial is flushed before the DCS starts
    // and the DCS content is untouched.
    //
    // Note: this passes on the pre-fix implementation too. It is regression
    // cover for behaviour the rewrite deliberately keeps, not a repro.
    const INTEREST: u8 = VT_PARSER_INTEREST_UTF8 | VT_PARSER_INTEREST_CSI | VT_PARSER_INTEREST_DCS;
    let mut utf8 = VTPushParser::new_with_interest::<INTEREST>();
    let utf8_events = collect_events_multiple(&mut utf8, &[b"\xe4\xb8", b"\x1bP1;2|data\x1b\\"]);

    let mut plain =
        VTPushParser::new_with_interest::<{ VT_PARSER_INTEREST_CSI | VT_PARSER_INTEREST_DCS }>();
    let plain_events = collect_events(&mut plain, b"\xe4\xb8\x1bP1;2|data\x1b\\");

    let join = |events: &[TestEvent]| {
        events
            .iter()
            .filter_map(|e| match e {
                TestEvent::Raw(s) | TestEvent::Utf8(s) => Some(s.clone()),
                _ => None,
            })
            .collect::<String>()
    };
    assert_eq!(join(&utf8_events), join(&plain_events));
    assert_eq!(utf8_events.last(), plain_events.last());
}

#[test]
fn test_utf8_partial_does_not_join_across_an_escape() {
    // A sequence left pending when an escape starts is flushed as Raw, so a
    // continuation arriving *after* the escape must not complete it. Joining
    // across would mean the escape sequence was skipped over.
    //
    // Note: this passes on the pre-fix implementation too — it guards against a
    // plausible wrong turn in the rewrite (carrying `utf8_partial` past an
    // escape), not against the original bug.
    const INTEREST: u8 = VT_PARSER_INTEREST_UTF8 | VT_PARSER_INTEREST_CSI;
    let mut parser = VTPushParser::new_with_interest::<INTEREST>();
    let events = collect_events_multiple(&mut parser, &[b"\xe4\xb8", b"\x1b[31m", b"\x96"]);

    assert_eq!(events.len(), 3, "{events:?}");
    assert_eq!(events[0], TestEvent::Raw("<e4b8>".to_string()));
    assert_eq!(
        events[1],
        TestEvent::Other("Csi('31', '', 'm')".to_string())
    );
    assert_eq!(events[2], TestEvent::Raw("<96>".to_string()));
}

#[test]
fn test_utf8_partial_at_c0() {
    // Test: feed("ab\xe4\xb8\x96\x07") - complete sequence then BEL (C0)
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, b"ab\xe4\xb8\x96\x07");

    // Should get: Raw("ab"), Utf8("世"), C0(0x07)
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], TestEvent::Raw("ab".to_string()));
    assert_eq!(events[1], TestEvent::Utf8("世".to_string()));
    assert_eq!(events[2], TestEvent::Other("C0(07)".to_string()));
}

#[test]
fn test_utf8_partial_dropped_at_c0() {
    // Test: feed("ab\xe4\xb8\x07") - incomplete sequence (e4 b8 needs 96) before BEL
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, b"ab\xe4\xb8\x07");

    // The bytes so far are not a character, so they travel as Raw: nothing is
    // dropped, and nothing is emitted out of order.
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], TestEvent::Raw("ab".to_string()));
    assert_eq!(events[1], TestEvent::Raw("<e4b8>".to_string()));
    assert_eq!(events[2], TestEvent::Other("C0(07)".to_string()));
}

#[test]
fn test_utf8_surrogate_rejected() {
    // U+D800 = ED A0 80 (surrogate) - should be rejected as invalid UTF-8
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, &[0xed, 0xa0, 0x80]);

    // Should emit as Raw (invalid), not Utf8. `encode_string` escapes each
    // invalid chunk separately, so the bytes show up one by one.
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], TestEvent::Raw("<ed><a0><80>".to_string()));
}

#[test]
fn test_utf8_overlong_rejected() {
    // Overlong encoding of '/' (0x2f) = C0 AF - should be rejected
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, &[0xc0, 0xaf]);

    // Should emit as Raw (invalid), not Utf8
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], TestEvent::Raw("<c0><af>".to_string()));
}

#[test]
fn test_utf8_out_of_range_rejected() {
    // U+110000 = F4 90 80 80 (out of range, max is U+10FFFF) - should be rejected
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let events = collect_events(&mut parser, &[0xf4, 0x90, 0x80, 0x80]);

    // Should emit as Raw (invalid), not Utf8
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], TestEvent::Raw("<f4><90><80><80>".to_string()));
}

#[test]
fn test_utf8_finish_drops_partial() {
    // A truncated sequence is not a character: finish() discards it rather than
    // inventing a `Raw` event for it.
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let mut events = Vec::new();
    parser.feed_with(b"ok\xe4\xb8", &mut |e: VTEvent<'_>| match e {
        VTEvent::Raw(s) => events.push(TestEvent::Raw(encode_string(s))),
        VTEvent::Utf8(s) => events.push(TestEvent::Utf8(encode_string(s))),
        _ => events.push(TestEvent::Other(format!("{:?}", e))),
    });
    assert_eq!(events, [TestEvent::Raw("ok".to_string())]);

    parser.finish(|e: VTEvent<'_>| match e {
        VTEvent::Raw(s) => events.push(TestEvent::Raw(encode_string(s))),
        VTEvent::Utf8(s) => events.push(TestEvent::Utf8(encode_string(s))),
        _ => events.push(TestEvent::Other(format!("{:?}", e))),
    });

    // Nothing more: the two held bytes are gone, not reported.
    assert_eq!(events, [TestEvent::Raw("ok".to_string())]);

    // The parser is clean afterwards: the next feed starts fresh, so the leftover
    // continuation byte is a stray byte rather than the tail of the dropped one.
    let after = collect_events(&mut parser, b"\x96!");
    assert_eq!(
        after,
        [
            TestEvent::Raw("<96>".to_string()),
            TestEvent::Raw("!".to_string())
        ]
    );
}

#[test]
fn test_utf8_abort_respected() {
    // The abort signal must stop the UTF-8 path too, not just the escape path.
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_UTF8>();
    let mut seen = 0;
    let processed = parser.feed_with_abortable(b"Hello \xe4\xb8\x96", &mut |_e: VTEvent<'_>| {
        seen += 1;
        false // stop after the first event
    });

    assert_eq!(seen, 1);
    // Only the bytes covered by that first event were consumed.
    assert_eq!(processed, 6);
}

/// Adjacent `Raw` events are merged: a chunk boundary may split one run of plain
/// text into two events, and that is not observable in any other way.
fn coalesce(events: Vec<TestEvent>) -> Vec<TestEvent> {
    let mut out: Vec<TestEvent> = Vec::with_capacity(events.len());
    for event in events {
        match (out.last_mut(), event) {
            (Some(TestEvent::Raw(prev)), TestEvent::Raw(next)) => prev.push_str(&next),
            (_, event) => out.push(event),
        }
    }
    out
}

const INTEREST: u8 = VT_PARSER_INTEREST_UTF8 | VT_PARSER_INTEREST_CSI | VT_PARSER_INTEREST_OTHER;

/// Every split of an input across feeds must produce the same events as feeding
/// it whole: the parser is a streaming parser, so chunk boundaries must not be
/// observable.
fn check_all_split_points(input: &[u8]) {
    // A parser can emit at most one event per input byte: every event either
    // consumes at least one byte of the input or carries the bytes of a sequence
    // that already did, and `finish()` adds at most one more. So for the inputs
    // used here (at most 24 bytes) the true ceiling is 25, and 64 cannot be
    // reached by a correct parser — it can only mean a runaway. See
    // `test_at_most_one_event_per_input_byte`, which pins that invariant.
    const CAP: usize = 64;

    let whole = {
        let mut parser = VTPushParser::new_with_interest::<INTEREST>();
        let mut events = Vec::new();
        capped(&mut parser, input, CAP, &mut events);
        parser.finish(|e: VTEvent<'_>| match e {
            VTEvent::Raw(s) => events.push(TestEvent::Raw(encode_string(s))),
            VTEvent::Utf8(s) => events.push(TestEvent::Utf8(encode_string(s))),
            _ => events.push(TestEvent::Other(format!("{:?}", e))),
        });
        coalesce(events)
    };

    for split in 1..input.len() {
        let mut parser = VTPushParser::new_with_interest::<INTEREST>();
        let mut events = Vec::new();
        capped(&mut parser, &input[..split], CAP, &mut events);
        capped(&mut parser, &input[split..], CAP, &mut events);
        parser.finish(|e: VTEvent<'_>| match e {
            VTEvent::Raw(s) => events.push(TestEvent::Raw(encode_string(s))),
            VTEvent::Utf8(s) => events.push(TestEvent::Utf8(encode_string(s))),
            _ => events.push(TestEvent::Other(format!("{:?}", e))),
        });
        assert_eq!(coalesce(events), whole, "split at {split} of {input:02x?}");
    }
}

/// The event cap used by the chunking tests is only safe if a correct parser
/// cannot legitimately reach it. That rests on one invariant: an event never
/// costs fewer than one input byte, so no input of `n` bytes can produce more
/// than `n + 1` events (the +1 being `finish()`).
///
/// Checked here against the bytes that emit the most events: stray
/// continuations and C0 controls, one event each. If this ever fails, the cap
/// in `check_all_split_points` needs raising before it can mask a real bug.
#[test]
fn test_at_most_one_event_per_input_byte() {
    for byte in [0x80u8, 0x81, 0x96, 0xbf, 0x07, 0x00, 0x7f, 0x1b] {
        for len in 1..=24 {
            let input = vec![byte; len];
            let mut parser = VTPushParser::new_with_interest::<INTEREST>();
            let mut events = Vec::new();
            capped(&mut parser, &input, 64, &mut events);
            parser.finish(|e: VTEvent<'_>| match e {
                VTEvent::Raw(s) => events.push(TestEvent::Raw(encode_string(s))),
                VTEvent::Utf8(s) => events.push(TestEvent::Utf8(encode_string(s))),
                _ => events.push(TestEvent::Other(format!("{:?}", e))),
            });
            assert!(
                events.len() <= len + 1,
                "{len} bytes of 0x{byte:02x} produced {} events",
                events.len()
            );
        }
    }
}

#[test]
fn test_utf8_chunking_is_transparent() {
    // Rejections, splits and terminators from the review's repros, plus plain
    // text, escapes and invalid sequences.
    //
    // Which of these actually catch a bug depends on where the interesting byte
    // sits relative to a feed boundary, so cases are kept in both positions
    // where that matters. Checked against the original implementation (4099c262):
    // 11 of the 21 cases fail there, the rest are regression cover only — do not
    // assume a case is load-bearing because it looks like the bug it names.
    let cases: &[&[u8]] = &[
        b"Hello \xe4\xb8\x96\xe7\x95\x8c",
        b"ab\xe4\xb8\x07",
        b"hi\xe4\xb8\x1b[A",
        b"\xf0\x9f\x98\x80!",
        b"\xed\xa0\x80",
        b"\xc0\xaf",
        b"\xf4\x90\x80\x80",
        b"\x80\x81\x82",
        // A lead byte at the start of a feed, followed by ASCII: the stale-state
        // shape. The same bytes mid-feed (`a\xc2b` below) do not expose it — the
        // divergence needs the lead byte first, so both positions are kept.
        b"\xc2ab",
        b"a\xc2b",
        b"\x1b[31mred\x1b[0m",
        // A lead byte or a partial left over from one feed meeting a terminator
        // or a second lead byte at the start of the next.
        b"\xe4\x07",
        b"\xe4\xb8\n",
        b"\xe4\xe4\xb8\x96",
        b"\xf0\x9f\xc3\xa9",
        b"\xe4\xb8\x1b[A",
        b"\xc2\x80",
        b"\xe0\x80\x80",
        b"\xf0\x80\x80\x80",
        // The review's hang: a stray continuation, a lead byte with nothing to
        // complete it, a BEL, then another stray continuation. The old loop
        // rewound its cursor here and never finished.
        b"\x96\xc2\x07\x80",
    ];
    for case in cases {
        check_all_split_points(case);
    }
}

/// A fixed-seed randomized version of the same property. The hand-written cases
/// above can only cover shapes somebody thought of; this walks a byte alphabet
/// weighted towards leads, continuations and terminators, and compares every
/// split point of every input against feeding it whole.
///
/// The seed is fixed so a failure is reproducible, and the iteration count is
/// sized to run in well under a second. It found a real divergence in the
/// original implementation within its first few inputs.
#[test]
fn test_utf8_chunking_is_transparent_fuzz() {
    // xorshift64: no dependency, good enough to cover a byte alphabet.
    fn next(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }
    let rand = |state: &mut u64, n: usize| (next(state) % n as u64) as usize;

    let alphabet: &[u8] = &[
        0x00, 0x07, 0x0a, 0x1b, 0x20, 0x41, 0x5b, 0x6d, 0x7f, 0x80, 0x81, 0x8f, 0x90, 0x96, 0x9f,
        0xa0, 0xbf, 0xc0, 0xc1, 0xc2, 0xc3, 0xdf, 0xe0, 0xe1, 0xed, 0xef, 0xf0, 0xf4, 0xf5, 0xff,
    ];

    let mut state = 0x243F_6A88_85A3_08D3u64;
    for _ in 0..2000 {
        let len = 1 + rand(&mut state, 12);
        let input: Vec<u8> = (0..len)
            .map(|_| alphabet[rand(&mut state, alphabet.len())])
            .collect();
        check_all_split_points(&input);
    }
}

/// Every possible two-byte input, at its one split point: no sampling, no
/// alphabet. This covers each lead byte followed by each possible next byte,
/// which is where `utf8_partial` carries state between feeds.
///
/// 65536 inputs in well under a tenth of a second, so there is no reason to
/// sample this shape.
#[test]
fn test_utf8_chunking_is_transparent_exhaustive_two_bytes() {
    for first in 0..=255u8 {
        for second in 0..=255u8 {
            check_all_split_points(&[first, second]);
        }
    }
}
