# vt-push-parser

A streaming push parser for the VT protocol.

This crate provides a push parser that can be fed bytes and will emit events as
they are parsed. You can easily use this as part of a push or pull pipeline.

## Zero-alloc

This crate will eventually be zero-alloc, but currently it is not zero-alloc
in all cases.

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
