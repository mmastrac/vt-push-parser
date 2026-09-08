//! Benchmarks for the `vt-input-push-parser` crate: decoding terminal input
//! (typed characters, named keys, SGR mouse reports and bracketed pastes).
use std::sync::LazyLock;

use vt_input_push_parser::{InputEvent, MouseEventKind, VTPushParserInput};

/// Number of times each corpus is repeated to build the input buffer.
const CORPUS_REPEAT: usize = 200;

/// Plain text, as typed by a user (including a few multi-byte characters).
static TEXT: &[u8] = "the quick brown fox jumps over the lazy dog — ⌘⇧\n".as_bytes();

/// Named keys and modified keys: cursor keys, SS3 function keys, CSI ~ keys and
/// alt-prefixed keys.
static KEYS: &[u8] =
    b"\x1b[A\x1b[B\x1b[C\x1b[D\x1bOP\x1bOQ\x1b[1;5A\x1b[3~\x1b[15;2~\x1b\x1b[Z\x1b[H\x1b[F\x7f";

/// SGR mouse reports: press, drag, release and motion.
static MOUSE: &[u8] = b"\x1b[<0;10;20M\x1b[<32;11;21M\x1b[<0;12;22m\x1b[<35;13;23M\x1b[<64;14;24M";

/// A bracketed paste of a small payload.
static PASTE: &[u8] = b"\x1b[200~pasted \x1b[not-an-escape text\n\x1b[201~";

static TEXT_CORPUS: LazyLock<Vec<u8>> = LazyLock::new(|| TEXT.repeat(CORPUS_REPEAT));
static KEYS_CORPUS: LazyLock<Vec<u8>> = LazyLock::new(|| KEYS.repeat(CORPUS_REPEAT));
static MOUSE_CORPUS: LazyLock<Vec<u8>> = LazyLock::new(|| MOUSE.repeat(CORPUS_REPEAT));
static PASTE_CORPUS: LazyLock<Vec<u8>> = LazyLock::new(|| PASTE.repeat(CORPUS_REPEAT));
static MIXED_CORPUS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut corpus = Vec::new();
    for _ in 0..CORPUS_REPEAT {
        corpus.extend_from_slice(TEXT);
        corpus.extend_from_slice(KEYS);
        corpus.extend_from_slice(MOUSE);
        corpus.extend_from_slice(PASTE);
    }
    corpus
});

/// Consume every event, accumulating a checksum so that nothing can be
/// optimized away.
fn accumulate(event: InputEvent, acc: &mut u64) {
    *acc = acc.wrapping_mul(31).wrapping_add(match event {
        InputEvent::Char(c) => c as u64,
        InputEvent::Key(key, modifiers) => key as u64 ^ (modifiers.bits() as u64) << 8,
        InputEvent::KeyChar(c, modifiers) => c as u64 ^ (modifiers.bits() as u64) << 24,
        InputEvent::Mouse(mouse) => {
            let kind = match mouse.kind {
                MouseEventKind::Press(_) => 1,
                MouseEventKind::Release(_) => 2,
                MouseEventKind::Drag(_) => 3,
                MouseEventKind::Motion => 4,
            };
            kind ^ (mouse.x as u64) << 8 ^ (mouse.y as u64) << 24
        }
        InputEvent::Paste(_, data) => data.len() as u64,
        InputEvent::Csi(csi) => csi.params.len() as u64,
        InputEvent::Osc(data) | InputEvent::Dcs(data) => data.len() as u64,
        _ => 0,
    });
}

macro_rules! make_bench {
    ($name:ident, $corpus:ident) => {
        #[divan::bench]
        fn $name(b: divan::Bencher) {
            let corpus = &*$corpus;
            b.bench(move || {
                let mut parser = VTPushParserInput::new();
                let mut acc = 0u64;
                parser.feed_with(corpus, |event| accumulate(event, &mut acc));
                divan::black_box_drop(acc);
                divan::black_box_drop(parser);
            });
        }
    };
}

make_bench!(input_text, TEXT_CORPUS);
make_bench!(input_keys, KEYS_CORPUS);
make_bench!(input_mouse, MOUSE_CORPUS);
make_bench!(input_paste, PASTE_CORPUS);
make_bench!(input_mixed, MIXED_CORPUS);

/// Feed the mixed corpus one byte at a time, which is the worst case for a
/// push parser reading from an unbuffered terminal: every escape sequence has
/// to be disambiguated across feed calls.
#[divan::bench]
fn input_mixed_bytewise(b: divan::Bencher) {
    let corpus = &*MIXED_CORPUS;
    b.bench(move || {
        let mut parser = VTPushParserInput::new();
        let mut acc = 0u64;
        for byte in corpus {
            parser.feed_with(&[*byte], |event| accumulate(event, &mut acc));
        }
        divan::black_box_drop(acc);
        divan::black_box_drop(parser);
    });
}

fn main() {
    divan::main();
}
