# vt-push-parser

[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://app.codspeed.io/mmastrac/vt-push-parser?utm_source=badge)

A streaming push parser for the VT protocol.

| Crate | Description | crates.io | docs.rs | Source |
|-------|-------------|-----------|---------|--------|
| `vt-push-parser` | Streaming VT push parser with input event parsing | [![crates.io](https://img.shields.io/crates/v/vt-push-parser)](https://crates.io/crates/vt-push-parser) | [![docs.rs](https://img.shields.io/docsrs/vt-push-parser)](https://docs.rs/vt-push-parser) | [vt-push-parser/](vt-push-parser/) |
| `fast-strip-ansi` | Fast ANSI escape sequence stripper | [![crates.io](https://img.shields.io/crates/v/fast-strip-ansi)](https://crates.io/crates/fast-strip-ansi) | [![docs.rs](https://img.shields.io/docsrs/fast-strip-ansi)](https://docs.rs/fast-strip-ansi) | [fast-strip-ansi/](fast-strip-ansi/) |
| `vt-input-push-parser` | Input event parser for VT sequences | [![crates.io](https://img.shields.io/crates/v/vt-input-push-parser)](https://crates.io/crates/vt-input-push-parser) | [![docs.rs](https://img.shields.io/docsrs/vt-input-push-parser)](https://docs.rs/vt-input-push-parser) | [vt-input-push-parser/](vt-input-push-parser/) |

All three crates are at version 0.15.0.

## Benchmarks

The benchmarks live in [`crates/benches`](crates/benches) and are written with
`divan` (through the CodSpeed compatibility layer, which behaves exactly like
`divan` when run outside of CodSpeed):

```bash
cargo bench -p benches
```

Every push and pull request is measured on
[CodSpeed](https://app.codspeed.io/mmastrac/vt-push-parser) using CPU
simulation, so performance changes show up as part of the review.
