# vt-push-parser

A streaming push parser for the VT protocol.

This crate provides a push parser that can be fed bytes and will emit events as
they are parsed. You can easily use this as part of a push or pull pipeline.

## Zero-alloc

Parsing is allocation-free. `feed_with` and `finish` make no heap allocations
for any input, including input that overruns the parameter caps, and including
sequences split across `feed_with` calls. Parameters are buffered in
fixed-capacity inline storage (`MAX_PARAMS`, `MAX_PARAM_LEN`, `MAX_PARAM_BYTES`),
so nothing has to grow. `tests/alloc_count.rs` enforces this with a counting
global allocator.

Anything that copies event data out allocates, by design, and is for use outside
the parsing hot path:

- `VTEvent::to_owned`, which produces `Vec<u8>` payloads for `Raw`/`OSC`/`DCS`
  data;
- `ParamBuf::to_owned`, which copies parameters into content-sized
  `ParamBufOwned` storage (two allocations once non-empty, none when empty —
  its size tracks the parameter data, not the caps).

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
