# vt-push-parser

[![CodSpeed](https://img.shields.io/endpoint?url=https://codspeed.io/badge.json)](https://app.codspeed.io/mmastrac/vt-push-parser?utm_source=badge)

A streaming push parser for the VT protocol.

There are a number of crates in this project:

 - [`vt-push-parser`](crates/vt-push-parser): A streaming push parser for the VT protocol.
 - [`vt-input-push-parser`](crates/vt-input-push-parser): A streaming push parser for VT input events.
 - [`fast-strip-ansi`](crates/fast-strip-ansi): A performance-optimized, low-allocation ANSI escape sequence stripper based on the `vt-push-parser` crate.

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
