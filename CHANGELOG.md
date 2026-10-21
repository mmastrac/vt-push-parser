# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **OSC/DCS**: an empty string payload no longer reports a stale slice in
  `OscEnd.data` / `DcsEnd`.
- **Input parser**: `VTPushParserInput::feed_with` spun forever when a chunk ended
  mid-escape (for example `\x1b]0;` followed by `title\x07`), so a public entry
  point never returned. It now returns once there is no input left for the parser
  to make progress on; the partial escape is held and resumes on the next feed.
- **Input parser**: an OSC payload split across feeds is accumulated instead of
  being emitted truncated from `OscEnd`.

### Changed

- Documented the payload contract for chunked input.

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