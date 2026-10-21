//! Allocation counting test for the zero-alloc parser.
//!
//! A counting global allocator verifies that the parsing hot path performs no
//! heap allocations: `feed_with` (whole-buffer and byte-at-a-time) and
//! `finish`. Copying data out of an event is a separate matter — `to_owned()`
//! allocates by design, and the tests below pin down *how much* rather than
//! asserting zero.
//!
//! Counting is per-thread, and each test measures only its own thread, so the
//! test harness printing from the main thread (or other tests running in
//! parallel) can't appear in a measurement. Inputs are built before the window
//! opens so only the parser's own behaviour is counted.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use vt_push_parser::{VT_PARSER_INTEREST_ALL, VTPushParser, event::VTEvent};

thread_local! {
    /// Whether allocations on this thread are being counted.
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    /// Allocations made on this thread since the window opened.
    static ALLOC_COUNT: Cell<usize> = const { Cell::new(0) };
    /// Bytes requested on this thread since the window opened.
    static ALLOC_BYTES: Cell<usize> = const { Cell::new(0) };
}

#[global_allocator]
static COUNTING_ALLOCATOR: CountingAllocator = CountingAllocator;

struct CountingAllocator;

/// Record one allocation on the current thread, if a window is open for it.
/// Uses `try_with` so allocations during TLS teardown are ignored.
fn record(size: usize) {
    MEASURING.with(|measuring| {
        if measuring.get() {
            ALLOC_COUNT.with(|c| c.set(c.get() + 1));
            ALLOC_BYTES.with(|b| b.set(b.get() + size));
        }
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Deliberately not counted: memory allocated outside a window can be
        // freed inside one, which would otherwise underflow the count.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// A measurement window on the current thread.
struct Window {
    open: bool,
}

impl Window {
    /// Start counting allocations made on this thread.
    fn open() -> Self {
        ALLOC_COUNT.with(|c| c.set(0));
        ALLOC_BYTES.with(|b| b.set(0));
        MEASURING.with(|m| m.set(true));
        Window { open: true }
    }

    /// Stop counting and return `(allocations, bytes)` made inside the window.
    fn close(mut self) -> (usize, usize) {
        MEASURING.with(|m| m.set(false));
        self.open = false;
        (ALLOC_COUNT.with(Cell::get), ALLOC_BYTES.with(Cell::get))
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // A panic inside the window must not leave counting enabled.
        if self.open {
            MEASURING.with(|m| m.set(false));
        }
    }
}

/// Write `n` in decimal into `buf`, returning the number of bytes written.
/// Allocation-free, so inputs can be built before the window opens.
fn write_u16(buf: &mut [u8], mut n: u16) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut digits = 0;
    while n > 0 {
        buf[digits] = b'0' + (n % 10) as u8;
        n /= 10;
        digits += 1;
    }
    buf[..digits].reverse();
    digits
}

/// Build `ESC [ 0;1;...;<count-1> m` followed by `trailing`, returning the
/// length written to `buf`.
fn build_many_params(buf: &mut [u8], count: u16, trailing: &[u8]) -> usize {
    let mut pos = 0;
    buf[pos] = 0x1b; // ESC
    pos += 1;
    buf[pos] = b'[';
    pos += 1;
    let mut digits = [0u8; 5];
    for i in 0..count {
        if i > 0 {
            buf[pos] = b';';
            pos += 1;
        }
        let len = write_u16(&mut digits, i);
        buf[pos..pos + len].copy_from_slice(&digits[..len]);
        pos += len;
    }
    buf[pos] = b'm';
    pos += 1;
    buf[pos..pos + trailing.len()].copy_from_slice(trailing);
    pos + trailing.len()
}

#[test]
fn window_counts_allocations() {
    // Guards the harness itself: if the counting window were broken (say, TLS
    // lookup failing silently), every no-alloc test above would pass vacuously.
    let window = Window::open();
    let v: Vec<u8> = Vec::with_capacity(64);
    let (allocs, bytes) = window.close();

    assert!(allocs >= 1, "window saw no allocations for a 64-byte Vec");
    assert!(bytes >= 64, "window saw {bytes} bytes for a 64-byte Vec");
    assert_eq!(v.capacity(), 64);
}

#[test]
fn no_alloc_on_clamped_bulk_feed() {
    // 200 params: past MAX_PARAMS (128), so the clamping path runs too.
    let mut buf = [0u8; 1024];
    let len = build_many_params(&mut buf, 200, b"after");
    let input = &buf[..len];

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    parser.feed_with(input, |_ev: VTEvent| {});
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "bulk feed allocated {allocs} times ({bytes} bytes)"
    );
    assert!(parser.is_ground());
}

#[test]
fn no_alloc_on_byte_by_byte_feed() {
    // Same sequence, fed one byte per call: the caps must hold across
    // `feed_with` boundaries without allocating.
    let mut buf = [0u8; 1024];
    let len = build_many_params(&mut buf, 200, b"");
    let input = &buf[..len];

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    for b in input {
        parser.feed_with(std::slice::from_ref(b), |_ev: VTEvent| {});
    }
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "byte-by-byte feed allocated {allocs} times ({bytes} bytes)"
    );
}

#[test]
fn no_alloc_on_subparams() {
    // Colon subparameters, plenty of them, still inline.
    let mut buf = [0u8; 2048];
    let mut pos = 0;
    buf[pos] = 0x1b;
    pos += 1;
    buf[pos] = b'[';
    pos += 1;
    let mut digits = [0u8; 5];
    for i in 0..100u16 {
        if i > 0 {
            buf[pos] = b';';
            pos += 1;
        }
        for (idx, value) in [i, i + 100, i + 200].into_iter().enumerate() {
            if idx > 0 {
                buf[pos] = b':';
                pos += 1;
            }
            let len = write_u16(&mut digits, value);
            buf[pos..pos + len].copy_from_slice(&digits[..len]);
            pos += len;
        }
    }
    buf[pos] = b'm';
    pos += 1;
    let input = &buf[..pos];

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    parser.feed_with(input, |_ev: VTEvent| {});
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "subparams allocated {allocs} times ({bytes} bytes)"
    );
}

#[test]
fn no_alloc_on_overlong_param() {
    // One 1024-byte parameter, clamped to MAX_PARAM_LEN (32).
    let mut buf = [0u8; 1024 + 3];
    let mut pos = 0;
    buf[pos] = 0x1b;
    pos += 1;
    buf[pos] = b'[';
    pos += 1;
    for _ in 0..1024 {
        buf[pos] = b'7';
        pos += 1;
    }
    buf[pos] = b'm';
    pos += 1;
    let input = &buf[..pos];

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    parser.feed_with(input, |_ev: VTEvent| {});
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "overlong param allocated {allocs} times ({bytes} bytes)"
    );
}

#[test]
fn no_alloc_on_dcs_and_osc() {
    let dcs_header = b"\x1bP1;2;3|";
    let dcs_body = b"payload\x1b\\";
    let osc_start = b"\x1b]10;rgb:fff/000/000";
    let osc_bel = b"\x07";
    let osc_st = b"\x1b]11;rgb:000/fff/000\x1b\\";

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    parser.feed_with(dcs_header, |_ev: VTEvent| {});
    parser.feed_with(dcs_body, |_ev: VTEvent| {});
    parser.feed_with(osc_start, |_ev: VTEvent| {});
    parser.feed_with(osc_bel, |_ev: VTEvent| {});
    parser.feed_with(osc_st, |_ev: VTEvent| {});
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "DCS/OSC feed allocated {allocs} times ({bytes} bytes)"
    );
}

#[test]
fn no_alloc_on_finish() {
    // Leave an unterminated OSC plus a held ESC in flight, then flush.
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    parser.feed_with(b"\x1b]0;title\x1b", |_ev: VTEvent| {});

    let window = Window::open();
    parser.finish(|_ev: VTEvent| {});
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "finish() allocated {allocs} times ({bytes} bytes)"
    );
    assert!(parser.is_ground());
}

#[test]
fn no_alloc_on_param_buf_to_owned_when_empty() {
    // `ParamBufOwned` is content-sized (`Option<Box<[u8]>>`): with no
    // parameters it is the shared empty value and allocates nothing.
    let input = b"\x1b[m";

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    let mut owned = None;
    parser.feed_with(input, |ev: VTEvent| {
        if let VTEvent::Csi(csi) = ev {
            owned = Some(csi.params.to_owned());
        }
    });
    let (allocs, bytes) = window.close();

    assert_eq!(
        allocs, 0,
        "ParamBuf::to_owned() on an empty list allocated {allocs} times ({bytes} bytes)"
    );
    let owned = owned.expect("no Csi event");
    assert!(owned.is_empty());
}

#[test]
fn param_buf_to_owned_allocs_do_not_scale_with_params() {
    // `to_owned()` *does* allocate once the list is non-empty: it copies into
    // content-sized storage (one box for the bytes, one for the lengths). The
    // property worth guarding is that this is bounded and independent of the
    // parameter count, not that it is zero.
    let input = b"\x1b[38:2:100:150:200m";

    let window = Window::open();
    let mut parser = VTPushParser::new_with_interest::<VT_PARSER_INTEREST_ALL>();
    let mut owned = None;
    parser.feed_with(input, |ev: VTEvent| {
        if let VTEvent::Csi(csi) = ev {
            owned = Some(csi.params.to_owned());
        }
    });
    // A second sequence with many more parameters must not allocate more.
    let mut wide = [0u8; 256];
    wide[0] = 0x1b;
    wide[1] = b'[';
    let mut pos = 2;
    for i in 0..64u16 {
        if i > 0 {
            wide[pos] = b';';
            pos += 1;
        }
        wide[pos] = b'1';
        pos += 1;
    }
    wide[pos] = b'm';
    pos += 1;
    let mut wide_owned = None;
    parser.feed_with(&wide[..pos], |ev: VTEvent| {
        if let VTEvent::Csi(csi) = ev {
            wide_owned = Some(csi.params.to_owned());
        }
    });
    let (allocs, _bytes) = window.close();

    let owned = owned.expect("no Csi event");
    let wide_owned = wide_owned.expect("no Csi event for wide sequence");
    assert!(!owned.overflowed());
    // `38:2:100:150:200` is a single parameter; colons stay in the bytes.
    assert_eq!(owned.len(), 1);
    assert_eq!(owned.get(0), Some(&b"38:2:100:150:200"[..]));
    assert_eq!(wide_owned.len(), 64);

    // Two `to_owned()` calls, two boxes each, regardless of parameter count.
    assert_eq!(
        allocs, 4,
        "to_owned() x2 allocated {allocs} times; expected 2 boxes each"
    );
}
