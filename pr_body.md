This PR implements UTF-8 handling for the VT push parser as described in Issue #6.

## Changes

### Core Implementation (`crates/vt-push-parser/src/lib.rs`)
- Added `VT_PARSER_INTEREST_UTF8` flag (value `0x02`) to enable UTF-8 sequence detection and emission
- Added UTF-8 tracking state fields to `VTPushParser`:
  - `utf8_expected_continuations`: tracks how many continuation bytes are needed
  - `utf8_start_idx`: tracks where a UTF-8 sequence started
  - `utf8_partial_buf`: 4-byte buffer for partial UTF-8 sequences across feed boundaries
  - `utf8_partial_len`: current length of partial buffer
- Implemented UTF-8 aware fast path in `State::Ground`:
  - Completes partial UTF-8 sequences from previous feed calls
  - Detects complete 2, 3, and 4-byte UTF-8 sequences and emits them as `VTEvent::Utf8`
  - Tracks incomplete sequences across feed boundaries
  - Handles stray/invalid continuation bytes by emitting as `Raw`
  - Resets UTF-8 state on escape sequences and parser reset

### Events (`crates/vt-push-parser/src/event.rs`)
- Added `VTEvent::Utf8(&[u8])` variant for emitting validated UTF-8 sequences

### Tests (`crates/vt-push-parser/tests/utf8_interest.rs`)
New comprehensive test file covering:
- Complete UTF-8 sequences in single feed
- Partial UTF-8 sequences split across multiple feeds (2, 3, and 4-byte sequences)
- Mixed ASCII and UTF-8 text
- UTF-8 with escape sequences
- Stray continuation bytes
- Invalid continuation bytes
- UTF-8 without the interest flag (everything emitted as Raw)

### Test Updates (`crates/vt-push-parser/tests/escapes.rs`)
- Added handling for `VTEvent::Utf8` in test callback macro

## Behavior

When `VT_PARSER_INTEREST_UTF8` is enabled:
- Complete UTF-8 sequences are emitted as `VTEvent::Utf8` events
- ASCII text and escape sequences are emitted as `VTEvent::Raw` events
- Partial UTF-8 sequences at feed boundaries are buffered and completed on subsequent feeds
- Invalid UTF-8 sequences are emitted as `Raw` bytes

When the flag is not enabled (default):
- All bytes are emitted as `VTEvent::Raw` events (existing behavior preserved)

This enables downstream consumers to efficiently handle UTF-8 text while still getting raw bytes for escape sequences and other VT protocol elements.