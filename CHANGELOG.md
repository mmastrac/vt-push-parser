# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **OSC/DCS**: an empty string payload no longer reports a stale slice in
  `OscEnd.data` / `DcsEnd`. The slice start was only set when the first payload
  byte was buffered, so an empty payload (e.g. `ESC ] BEL`) repeated whatever
  came before it — the introducer, preceding text, or the previous OSC's
  payload — duplicating those bytes in the event stream and breaking
  `encode()` round-tripping.

### Changed
- Documented the payload contract for chunked input: OSC/DCS payload may arrive
  split across `OscData`/`DcsData` events, and `OscEnd.data`/`DcsEnd` may be
  empty. Concatenate the string's `*Data` bytes with its `*End` bytes to get the
  whole payload.

## [0.15.0] - 2025-10-20

### Changed
- **Core parser**: Fixed-capacity inline parameter storage (#15) — `ParamBufOwned` reduced from 1158 to 40 bytes, `Box<[u8]>` owned params, 45 new tests added
- **Benchmarking**: Added CodSpeed continuous benchmarking (#12)

## [0.14.0] - 2025-10-20

### Changed
- Bump workspace version 0.13.1 → 0.14.0 (#14)

## [0.13.1] - 2025-10-20

### Fixed
- `finish()`: flush held ESC as data and cancel in-flight OSC/DCS strings (#13)