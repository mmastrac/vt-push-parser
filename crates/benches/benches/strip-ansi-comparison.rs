use std::sync::LazyLock;

const CORPUS_REPEAT: usize = 10;

static NO_ANSI_CORPUS: &str = include_str!("no-ansi-corpus.txt");
static ESCAPE_SEQUENCES: &str = include_str!("escape-sequences.txt");
static MIXED: &str = include_str!("mixed.txt");

static ANSI_ZERO_PERCENT: LazyLock<String> = LazyLock::new(|| corpus(0));
static ANSI_10_PERCENT: LazyLock<String> = LazyLock::new(|| corpus(10));
static ANSI_25_PERCENT: LazyLock<String> = LazyLock::new(|| corpus(25));
static ANSI_50_PERCENT: LazyLock<String> = LazyLock::new(|| corpus(50));
static ANSI_75_PERCENT: LazyLock<String> = LazyLock::new(|| corpus(75));
static ANSI_100_PERCENT: LazyLock<String> = LazyLock::new(|| corpus(100));

static ESCAPE_CHOICES: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| {
    ESCAPE_SEQUENCES
        .lines()
        .map(|line| {
            vt_push_parser::ascii::decode_string(line)
                .as_slice()
                .to_vec()
        })
        .collect::<Vec<_>>()
});

/// Generate a corpus with `percent` of the chunks having ANSI escape sequences.
fn corpus(percent: u8) -> String {
    let corpus_chunk_size = NO_ANSI_CORPUS.len() * CORPUS_REPEAT / 100;
    let mut i = 0;
    let mut escape = 0;

    let mut collector = Vec::new();

    for chunk in NO_ANSI_CORPUS
        .repeat(CORPUS_REPEAT)
        .as_bytes()
        .chunks(corpus_chunk_size)
    {
        collector.extend_from_slice(chunk);
        if i < percent {
            escape += 1;
            if escape >= ESCAPE_CHOICES.len() {
                escape = 0;
            }
            collector.extend_from_slice(&ESCAPE_CHOICES[escape]);
        }
        i = (i + 1) % 100;
    }

    String::from_utf8_lossy(&collector).to_string()
}

macro_rules! make_bench {
    ($name:ident, $corpus:ident, $bencher:ident) => {
        #[divan::bench]
        fn $name(b: divan::Bencher) {
            let corpus = &*$corpus;
            b.bench(move || {
                $bencher(&corpus);
            });
        }
    };
}

#[inline(always)]
fn strip_ansi_crate(corpus_str: &str) -> usize {
    let output = strip_ansi::strip_ansi(corpus_str);
    let res = output.len();
    std::hint::black_box(output);
    res
}

#[inline(always)]
fn strip_ansi_escapes_crate(corpus_str: &str) -> usize {
    let output = strip_ansi_escapes::strip(corpus_str.as_bytes());
    let res = output.len();
    std::hint::black_box(output);
    res
}

#[inline(always)]
fn fast_strip_ansi_crate(corpus_str: &str) -> usize {
    let output = fast_strip_ansi::strip_ansi_string(corpus_str);
    let res = output.len();
    std::hint::black_box(output);
    res
}

#[inline(always)]
fn fast_strip_ansi_crate_bytes(corpus_str: &str) -> usize {
    let output = fast_strip_ansi::strip_ansi_bytes(corpus_str.as_bytes());
    let res = output.len();
    std::hint::black_box(output);
    res
}

#[inline(always)]
fn fast_strip_ansi_crate_callback(corpus_str: &str) -> usize {
    let mut len = 0;
    fast_strip_ansi::strip_ansi_bytes_callback(corpus_str.as_bytes(), |text| {
        len += text.len();
        std::hint::black_box(text);
    });
    len
}

fn main() {
    let expected_len = NO_ANSI_CORPUS.len() * CORPUS_REPEAT;

    if strip_ansi_crate(&ANSI_100_PERCENT) != expected_len {
        println!(
            "WARNING: strip_ansi_crate: {} != {expected_len}",
            strip_ansi_crate(&ANSI_100_PERCENT)
        );
    }
    if strip_ansi_escapes_crate(&ANSI_100_PERCENT) != expected_len {
        println!(
            "WARNING: strip_ansi_escapes_crate: {} != {expected_len}",
            strip_ansi_escapes_crate(&ANSI_100_PERCENT)
        );
    }
    if fast_strip_ansi_crate(&ANSI_100_PERCENT) != expected_len {
        println!(
            "WARNING: fast_strip_ansi_crate: {} != {expected_len}",
            fast_strip_ansi_crate(&ANSI_100_PERCENT)
        );
    }
    if fast_strip_ansi_crate_bytes(&ANSI_100_PERCENT) != expected_len {
        println!(
            "WARNING: fast_strip_ansi_crate_bytes: {} != {expected_len}",
            fast_strip_ansi_crate_bytes(&ANSI_100_PERCENT)
        );
    }

    eprintln!("{:?}", std::env::args());

    divan::main();
}

make_bench!(strip_ansi_crate_0, ANSI_ZERO_PERCENT, strip_ansi_crate);
make_bench!(strip_ansi_crate_10, ANSI_10_PERCENT, strip_ansi_crate);
make_bench!(strip_ansi_crate_25, ANSI_25_PERCENT, strip_ansi_crate);
make_bench!(strip_ansi_crate_50, ANSI_50_PERCENT, strip_ansi_crate);
make_bench!(strip_ansi_crate_75, ANSI_75_PERCENT, strip_ansi_crate);
make_bench!(strip_ansi_crate_100, ANSI_100_PERCENT, strip_ansi_crate);

make_bench!(
    strip_ansi_escapes_crate_0,
    ANSI_ZERO_PERCENT,
    strip_ansi_escapes_crate
);
make_bench!(
    strip_ansi_escapes_crate_10,
    ANSI_10_PERCENT,
    strip_ansi_escapes_crate
);
make_bench!(
    strip_ansi_escapes_crate_25,
    ANSI_25_PERCENT,
    strip_ansi_escapes_crate
);
make_bench!(
    strip_ansi_escapes_crate_50,
    ANSI_50_PERCENT,
    strip_ansi_escapes_crate
);
make_bench!(
    strip_ansi_escapes_crate_75,
    ANSI_75_PERCENT,
    strip_ansi_escapes_crate
);
make_bench!(
    strip_ansi_escapes_crate_100,
    ANSI_100_PERCENT,
    strip_ansi_escapes_crate
);

make_bench!(
    fast_strip_ansi_crate_0,
    ANSI_ZERO_PERCENT,
    fast_strip_ansi_crate
);
make_bench!(
    fast_strip_ansi_crate_10,
    ANSI_10_PERCENT,
    fast_strip_ansi_crate
);
make_bench!(
    fast_strip_ansi_crate_25,
    ANSI_25_PERCENT,
    fast_strip_ansi_crate
);
make_bench!(
    fast_strip_ansi_crate_50,
    ANSI_50_PERCENT,
    fast_strip_ansi_crate
);
make_bench!(
    fast_strip_ansi_crate_75,
    ANSI_75_PERCENT,
    fast_strip_ansi_crate
);
make_bench!(
    fast_strip_ansi_crate_100,
    ANSI_100_PERCENT,
    fast_strip_ansi_crate
);

make_bench!(
    fast_strip_ansi_crate_bytes_0,
    ANSI_ZERO_PERCENT,
    fast_strip_ansi_crate_bytes
);
make_bench!(
    fast_strip_ansi_crate_bytes_10,
    ANSI_10_PERCENT,
    fast_strip_ansi_crate_bytes
);
make_bench!(
    fast_strip_ansi_crate_bytes_25,
    ANSI_25_PERCENT,
    fast_strip_ansi_crate_bytes
);
make_bench!(
    fast_strip_ansi_crate_bytes_50,
    ANSI_50_PERCENT,
    fast_strip_ansi_crate_bytes
);
make_bench!(
    fast_strip_ansi_crate_bytes_75,
    ANSI_75_PERCENT,
    fast_strip_ansi_crate_bytes
);
make_bench!(
    fast_strip_ansi_crate_bytes_100,
    ANSI_100_PERCENT,
    fast_strip_ansi_crate_bytes
);

make_bench!(
    fast_strip_ansi_crate_callback_0,
    ANSI_ZERO_PERCENT,
    fast_strip_ansi_crate_callback
);
make_bench!(
    fast_strip_ansi_crate_callback_10,
    ANSI_10_PERCENT,
    fast_strip_ansi_crate_callback
);
make_bench!(
    fast_strip_ansi_crate_callback_25,
    ANSI_25_PERCENT,
    fast_strip_ansi_crate_callback
);
make_bench!(
    fast_strip_ansi_crate_callback_50,
    ANSI_50_PERCENT,
    fast_strip_ansi_crate_callback
);
make_bench!(
    fast_strip_ansi_crate_callback_75,
    ANSI_75_PERCENT,
    fast_strip_ansi_crate_callback
);
make_bench!(
    fast_strip_ansi_crate_callback_100,
    ANSI_100_PERCENT,
    fast_strip_ansi_crate_callback
);
