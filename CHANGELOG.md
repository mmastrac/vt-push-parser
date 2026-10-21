# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`VT_PARSER_INTEREST_UTF8`**: opt-in `VTEvent::Utf8` events, one per valid
  multi-byte UTF-8 sequence in ground text, so callers can tell real characters
  from stray bytes. ASCII still arrives as `Raw`; bytes that are not valid UTF-8
  (stray continuation bytes, overlong or surrogate encodings, out-of-range
  sequences) arrive as `Raw` too, so nothing is lost. A sequence split across
  `feed_with` calls is completed on the next call. `finish()` drops a truncated
  sequence rather than reporting it. `VT_PARSER_INTEREST_ALL` and
  `VT_PARSER_INTEREST_DEFAULT` deliberately exclude the flag, since it changes
  the shape of ground text rather than only adding events; ask for
  `VT_PARSER_INTEREST_ALL | VT_PARSER_INTEREST_UTF8` explicitly.

### Fixed

- **OSC/DCS**: an empty string payload no longer reports a stale slice in
  `OscEnd.data` / `DcsEnd`.

### Changed

- Documented the payload contract for chunked input.

### Breaking

- `VTEvent` and `VTOwnedEvent` gained a `Utf8` variant. The variant exists on
  the enum whether or not a caller opts in to `VT_PARSER_INTEREST_UTF8`, so any
  downstream code that matches `VTEvent` exhaustively stops compiling until it
  handles (or wildcards) the new arm. Neither enum is `#[non_exhaustive]`. This
  is a breaking change and needs a minor version bump (0.16.0 under 0.x semver)
  rather than a 0.15.x patch. Callers that only read `Raw`/`C0`/`Csi` and similar
  are unaffected at runtime: without the flag the events are the same as before.

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
