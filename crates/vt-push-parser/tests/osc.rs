//! Regression tests for OSC payload handling.
//!
//! An OSC stream with an empty payload used to report a stale slice as
//! `OscEnd.data`: whatever the parser had buffered last, i.e. the introducer,
//! preceding ground text, or even the previous OSC's payload. Since `encode()`
//! writes `data` back out, those duplicated bytes corrupted any stream rebuilt
//! from the events.
//!
//! Each `empty_osc_*` test below is one row of the reported table; every one of
//! them fails on `58c84fb8`.

use pretty_assertions::assert_eq;
use vt_push_parser::VTPushParser;
use vt_push_parser::event::{VTEvent, VTOwnedEvent};

/// Feed `input` in one go and collect the owned events.
fn events(input: &[u8]) -> Vec<VTOwnedEvent> {
    let mut parser = VTPushParser::new();
    let mut events = Vec::new();
    parser.feed_with(input, &mut |event: VTEvent| events.push(event.to_owned()));
    events
}

/// The `(data, used_bel)` of every `OscEnd` emitted for `input`.
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
    // ESC ] BEL: `data` used to be the introducer (`ESC ]`).
    assert_eq!(osc_ends(b"\x1b]\x07"), vec![EMPTY_BEL]);
}

#[test]
fn empty_osc_after_raw() {
    // abc ESC ] BEL: `data` used to be `abc ESC ]`.
    assert_eq!(osc_ends(b"abc\x1b]\x07"), vec![EMPTY_BEL]);
}

#[test]
fn empty_osc_after_csi() {
    // ESC [1m ESC ] BEL: `data` used to be `ESC [1m ESC ]`.
    assert_eq!(osc_ends(b"\x1b[1m\x1b]\x07"), vec![EMPTY_BEL]);
}

#[test]
fn empty_osc_after_previous_osc() {
    // ESC ]0;A BEL ESC ] BEL: the second `data` used to repeat the first
    // payload, then the BEL and the introducer: the start of the slice was
    // stale, not merely off by the introducer.
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
    // The ST forms: a held ESC set the slice start, so these were already
    // right, but they pin the behaviour down alongside the BEL forms.
    assert_eq!(osc_ends(b"\x1b]\x1b\\"), vec![EMPTY_ST]);
    assert_eq!(osc_ends(b"abc\x1b]\x1b\\"), vec![EMPTY_ST]);
    assert_eq!(
        osc_ends(b"\x1b]0;A\x1b\\\x1b]\x1b\\"),
        vec![(b"0;A".to_vec(), false), EMPTY_ST]
    );
}

#[test]
fn empty_dcs_payload_reports_empty_data() {
    // DcsEnd used the same stale-index pattern. An empty DCS payload always
    // terminates through a held ESC, so it was correct by accident; keep it so.
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
    // Control cases: these were already correct before the fix.
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
    // C0 bytes other than BEL/ESC inside an OSC string are dropped by the
    // parser (pre-existing policy, unrelated to this bug). An empty payload
    // therefore stays empty: it must not reach back over the introducer and
    // pick up the dropped C0.
    assert_eq!(osc_ends(b"\x1b]\n\x07"), vec![EMPTY_BEL]);
}

/// Encode the events of `chunks` back into a byte stream.
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
    // Every event must re-encode to the bytes that produced it, so a caller can
    // rebuild the stream from the events. This is what the empty-payload bug
    // broke: the duplicated `OscEnd.data` came straight back out of `encode()`.
    //
    // The corpus holds no truncated escape sequence (the parser drops those by
    // design) and no C0 bytes inside OSC/DCS strings (dropped as well, see
    // `ignored_c0_bytes_are_not_pulled_into_the_payload`).
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

        // The same input fed one byte at a time. A payload may arrive split
        // across `OscData`/`DcsData` when the input is chunked (see the
        // `VTEvent` docs); the bytes and their order must still be identical.
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let stepped = re_encode(&chunks);
        assert_eq!(
            stepped, input,
            "re-encoding {input:02X?} fed byte by byte gave {stepped:02X?}"
        );
    }
}
