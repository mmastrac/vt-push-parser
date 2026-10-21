//! Regression tests for OSC payload handling.

use pretty_assertions::assert_eq;
use vt_push_parser::VTPushParser;
use vt_push_parser::event::{VTEvent, VTOwnedEvent};

/// Feed `input` at once, collect the owned events.
fn events(input: &[u8]) -> Vec<VTOwnedEvent> {
    let mut parser = VTPushParser::new();
    let mut events = Vec::new();
    parser.feed_with(input, &mut |event: VTEvent| events.push(event.to_owned()));
    events
}

/// Every `OscEnd`'s `(data, used_bel)` for `input`.
fn osc_ends(input: &[u8]) -> Vec<(Vec<u8>, bool)> {
    events(input)
        .into_iter()
        .filter_map(|event| match event {
            VTOwnedEvent::OscEnd { data, used_bel } => Some((data, used_bel)),
            _ => None,
        })
        .collect()
}

const EMPTY_BEL: (Vec<u8>, bool) = (Vec::new(), true);
const EMPTY_ST: (Vec<u8>, bool) = (Vec::new(), false);

#[test]
fn empty_osc_after_introducer() {
    // ESC ] BEL: `data` was the introducer.
    assert_eq!(osc_ends(b"\x1b]\x07"), vec![EMPTY_BEL]);
}

#[test]
fn empty_osc_after_raw() {
    // `data` was `abc ESC ]`.
    assert_eq!(osc_ends(b"abc\x1b]\x07"), vec![EMPTY_BEL]);
}

#[test]
fn empty_osc_after_csi() {
    // `data` was `ESC [1m ESC ]`.
    assert_eq!(osc_ends(b"\x1b[1m\x1b]\x07"), vec![EMPTY_BEL]);
}

#[test]
fn empty_osc_after_previous_osc() {
    // Second `data` repeated the first payload: stale slice start.
    assert_eq!(
        osc_ends(b"\x1b]0;A\x07\x1b]\x07"),
        vec![(b"0;A".to_vec(), true), EMPTY_BEL]
    );
    // Same with three OSCs in a row.
    assert_eq!(
        osc_ends(b"\x1b]0;A\x07\x1b]1;B\x07\x1b]\x07"),
        vec![(b"0;A".to_vec(), true), (b"1;B".to_vec(), true), EMPTY_BEL]
    );
}

#[test]
fn empty_osc_terminated_by_st() {
    // ST forms: held ESC set the start already.
    assert_eq!(osc_ends(b"\x1b]\x1b\\"), vec![EMPTY_ST]);
    assert_eq!(osc_ends(b"abc\x1b]\x1b\\"), vec![EMPTY_ST]);
    assert_eq!(
        osc_ends(b"\x1b]0;A\x1b\\\x1b]\x1b\\"),
        vec![(b"0;A".to_vec(), false), EMPTY_ST]
    );
}

#[test]
fn empty_dcs_payload_reports_empty_data() {
    // Same pattern, correct only via the held ESC.
    let dcs_ends = |input: &[u8]| {
        events(input)
            .into_iter()
            .filter_map(|event| match event {
                VTOwnedEvent::DcsEnd(data) => Some(data),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(dcs_ends(b"\x1bPq\x1b\\"), vec![Vec::new()]);
    assert_eq!(dcs_ends(b"abc\x1bPq\x1b\\"), vec![Vec::new()]);
    assert_eq!(
        dcs_ends(b"\x1bPqA\x1b\\\x1bPq\x1b\\"),
        vec![b"A".to_vec(), Vec::new()]
    );
}

#[test]
fn non_empty_osc_payload_is_unaffected() {
    // Control cases, already correct before the fix.
    assert_eq!(osc_ends(b"abc\x1b]A\x07"), vec![(b"A".to_vec(), true)]);
    assert_eq!(
        osc_ends(b"\x1b]0;title\x07"),
        vec![(b"0;title".to_vec(), true)]
    );
    assert_eq!(
        osc_ends(b"\x1b]0;title\x1b\\"),
        vec![(b"0;title".to_vec(), false)]
    );
}

#[test]
fn ignored_c0_bytes_are_not_pulled_into_the_payload() {
    // C0 bytes inside OSC are dropped (pre-existing policy).
    assert_eq!(osc_ends(b"\x1b]\n\x07"), vec![EMPTY_BEL]);
}

#[test]
fn flushed_payload_is_not_repeated_at_the_end() {
    // DEL flushes early, leaving nothing for the end.
    assert_eq!(
        events(b"\x1b]A\x7f\x07"),
        vec![
            VTOwnedEvent::OscStart,
            VTOwnedEvent::OscData(b"A".to_vec()),
            VTOwnedEvent::OscEnd {
                data: Vec::new(),
                used_bel: true,
            },
        ]
    );
}

/// Encode `chunks`' events back to bytes.
fn re_encode(chunks: &[&[u8]]) -> Vec<u8> {
    let mut parser = VTPushParser::new();
    let mut out = Vec::new();
    for chunk in chunks {
        parser.feed_with(chunk, &mut |event: VTEvent| {
            let mut buf = [0_u8; 256];
            let len = event
                .encode(&mut buf)
                .unwrap_or_else(|len| panic!("event {event:?} needs {len} bytes"));
            out.extend_from_slice(&buf[..len]);
        });
    }
    out
}

#[test]
fn events_re_encode_byte_for_byte() {
    // Re-encoding every event must reproduce the input bytes.
    //
    // Corpus excludes truncated escapes and C0 bytes inside strings.
    let corpus: &[&[u8]] = &[
        b"",
        b"plain text",
        b"\x1b]\x07",
        b"abc\x1b]\x07",
        b"\x1b[1m\x1b]\x07",
        b"\x1b]0;A\x07\x1b]\x07",
        b"\x1b]\x07\x1b]\x07\x1b]\x07",
        b"\x1b]0;title\x07",
        b"\x1b]0;title\x1b\\",
        b"\x1b]\x1b\\",
        b"abc\x1b]A\x07",
        b"before\x1b]8;;https://example.com\x07link\x1b]8;;\x07after",
        b"\x1bPqdata\x1b\\",
        b"\x1bPq\x1b\\",
        b"abc\x1bPq\x1b\\",
        b"\x1b[32mgreen\x1b[0m",
        b"a\x07b",
    ];

    for &input in corpus {
        let whole = re_encode(&[input]);
        assert_eq!(
            whole, input,
            "re-encoding {input:02X?} fed whole gave {whole:02X?}"
        );

        // Also fed byte by byte, where payloads may split.
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let stepped = re_encode(&chunks);
        assert_eq!(
            stepped, input,
            "re-encoding {input:02X?} fed byte by byte gave {stepped:02X?}"
        );
    }
}
