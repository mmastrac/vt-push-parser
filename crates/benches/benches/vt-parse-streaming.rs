//! Benchmarks for the core `vt-push-parser` APIs: one-shot decoding, chunked
//! (streaming) feeding across arbitrary boundaries and the allocating iterator
//! wrapper.
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use vt_push_parser::VTEventCallback;
use vt_push_parser::VTPushParser;
use vt_push_parser::event::VTEvent;
use vt_push_parser::iter::VTIterator;

/// Number of times the mixed corpus is repeated to build the input buffer.
const CORPUS_REPEAT: usize = 100;

static MIXED: &str = include_str!("mixed.txt");

static CORPUS: LazyLock<Vec<u8>> =
    LazyLock::new(|| vt_push_parser::ascii::decode_string(MIXED).repeat(CORPUS_REPEAT));

/// A sink that hashes every event so that the parser output cannot be optimized
/// away.
#[derive(Default)]
struct HashSink {
    hash: std::hash::DefaultHasher,
}

impl VTEventCallback for &'_ mut HashSink {
    fn event(&mut self, event: VTEvent) {
        event.hash(&mut self.hash);
    }
}

/// Feed the whole corpus in a single call.
#[divan::bench]
fn feed_whole(b: divan::Bencher) {
    let corpus = &*CORPUS;
    b.bench(move || {
        let mut parser = VTPushParser::new();
        let mut sink = HashSink::default();
        parser.feed_with(corpus, &mut sink);
        divan::black_box_drop(sink.hash.finish());
        divan::black_box_drop(parser);
    });
}

/// Feed the corpus in fixed-size chunks, which forces the parser to carry state
/// across chunk boundaries.
#[divan::bench(args = [1, 16, 256])]
fn feed_chunked(b: divan::Bencher, chunk_size: usize) {
    let corpus = &*CORPUS;
    b.bench(move || {
        let mut parser = VTPushParser::new();
        let mut sink = HashSink::default();
        for chunk in corpus.chunks(chunk_size) {
            parser.feed_with(chunk, &mut sink);
        }
        divan::black_box_drop(sink.hash.finish());
        divan::black_box_drop(parser);
    });
}

/// One-shot decoding of a complete buffer.
#[divan::bench]
fn decode_buffer(b: divan::Bencher) {
    let corpus = &*CORPUS;
    b.bench(move || {
        let mut hash = std::hash::DefaultHasher::default();
        VTPushParser::decode_buffer(corpus, |event| {
            event.hash(&mut hash);
        });
        divan::black_box_drop(hash.finish());
    });
}

/// The convenience iterator wrapper, which allocates an owned event per item.
#[divan::bench]
fn iterator(b: divan::Bencher) {
    let corpus = &*CORPUS;
    b.bench(move || {
        let mut hash = std::hash::DefaultHasher::default();
        for event in VTIterator::new(corpus.chunks(256)) {
            event.borrow().hash(&mut hash);
        }
        divan::black_box_drop(hash.finish());
    });
}

fn main() {
    divan::main();
}
