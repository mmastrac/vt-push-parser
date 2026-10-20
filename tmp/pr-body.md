## Summary

Refactors `ParamBufOwned` to store concatenated param bytes and per-param lengths as `Option<Box<[u8]>>` instead of fixed inline arrays. This makes the struct **content-sized** (allocates exactly what's needed) and **zero-allocation when empty**.

## Changes

- `ParamBufOwned`: `Option<Box<[u8]>>` (params) + `Option<Box<[u8]>>` (lengths) + `overflowed: bool` — **1158 → 40 bytes**
- Internal `ParamSource<'a>` enum unifies parser inline storage and owned heap storage
- All public APIs unchanged: `ParamBuf`, `NumericParamBuf`, `ParamListIter`, `overflowed()`, `to_owned()`, `borrow()`, `numeric()`, serde
- Parser hot path (`ParamList`, zero-alloc clamping) completely untouched
- serde wire shape preserved: `{"params":[...]}`

## Verification

- `cargo test --workspace --features serde`: all pass (45 new tests added)
- `cargo fmt` / `cargo clippy`: clean
- Benchmarks (interleaved A/B): no regressions; `iterator` bench (owned events) **~23% faster** (68.9 vs 88.9 µs); core streaming & input benches flat

## Testing

Added 45 tests covering:
- Content-sizing behavior (empty, small, large)
- Clamping parity with inline `ParamBuf`
- Cross-view equality (`borrow()` vs `to_owned()`)
- Overflow flag propagation
- Size bounds assertions
- Serde wire shape preservation