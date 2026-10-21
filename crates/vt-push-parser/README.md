# vt-push-parser

A streaming push parser for the VT protocol.

This crate provides a push parser that can be fed bytes and will emit events as
they are parsed. You can easily use this as part of a push or pull pipeline.

## Zero-alloc

This crate will eventually be zero-alloc, but currently it is not zero-alloc
in all cases.

## Streamed payloads

Events borrow from the buffer passed to `feed_with`, so an event never spans two
calls. Text and string payloads may therefore be split across several events,
and the split depends on how the input was chunked:

- `Raw` runs are coalesced only within one call.
- For OSC and DCS strings, bytes flushed before the string ends arrive as
  `OscData`/`DcsData`; `OscEnd.data`/`DcsEnd` carry whatever is still buffered
  when the terminator arrives, and may be empty.

To get a whole payload, concatenate that string's `OscData` bytes followed by
its `OscEnd.data` bytes (likewise for DCS). Re-encoding every event in order
reproduces the input byte for byte, however it was chunked, as long as the
input contains no truncated escape sequence.

```rust
use vt_push_parser::event::VTEvent;
use vt_push_parser::VTPushParser;

let mut payload = Vec::new();
let mut parser = VTPushParser::new();
// Fed whole, this puts the payload in `OscEnd`; fed in pieces it does not.
for chunk in [&b"\x1b]0;ti"[..], &b"tle\x07"[..]] {
    parser.feed_with(chunk, |event: VTEvent| {
        match event {
            VTEvent::OscData(data) => payload.extend_from_slice(data),
            VTEvent::OscEnd { data, .. } => payload.extend_from_slice(data),
            _ => {}
        }
    });
}
assert_eq!(payload, b"0;title");
```

## Example

```rust
use vt_push_parser::{VTPushParser, event::VTEvent};

let mut parser = VTPushParser::new();

// Parse ANSI colored text
parser.feed_with(b"\x1b[32mHello\x1b[0m, world!", |event: VTEvent| {
    match event {
        VTEvent::Csi(csi) if csi.final_byte == b'm' => {
            println!("SGR sequence: {:?}", csi.params);
        }
        VTEvent::Raw(text) => {
            println!("Text: {}", String::from_utf8_lossy(text));
        }
        _ => {}
    }
});
```
