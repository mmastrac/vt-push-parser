//! This is not a scientific comparison, but more of a sanity check that perf is
//! not wildly slower than the other popular crate(s).
use std::hash::{Hash, Hasher};

const CORPUS_REPEAT: usize = 10000;

static MIXED: &str = include_str!("mixed.txt");

#[divan::bench]
fn vte_parse_mixed(b: divan::Bencher) {
    let corpus = vt_push_parser::ascii::decode_string(MIXED);
    b.bench(move || {
        #[derive(Default)]
        struct Process {
            hash: std::hash::DefaultHasher,
        }

        impl vte::Perform for Process {
            fn print(&mut self, c: char) {
                c.hash(&mut self.hash);
            }
            fn execute(&mut self, b: u8) {
                b.hash(&mut self.hash);
            }
            fn esc_dispatch(&mut self, intermediates: &[u8], _private: bool, final_byte: u8) {
                intermediates.hash(&mut self.hash);
                final_byte.hash(&mut self.hash);
            }
            fn csi_dispatch(
                &mut self,
                params: &vte::Params,
                intermediates: &[u8],
                _private: bool,
                final_byte: char,
            ) {
                params.len().hash(&mut self.hash);
                intermediates.len().hash(&mut self.hash);
                final_byte.hash(&mut self.hash);
            }
            fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
                params.hash(&mut self.hash);
            }
        }

        let mut parser = vte::Parser::new();
        let mut process = Process::default();
        for _ in 0..CORPUS_REPEAT {
            parser.advance(&mut process, &corpus);
        }

        divan::black_box_drop(parser);
    });
}

#[divan::bench]
fn vte_parse_mixed_sum(b: divan::Bencher) {
    let corpus = vt_push_parser::ascii::decode_string(MIXED);
    b.bench(move || {
        #[derive(Default)]
        struct Process {
            hash: std::hash::DefaultHasher,
        }

        impl vte::Perform for Process {
            fn print(&mut self, c: char) {
                c.hash(&mut self.hash);
            }
            fn csi_dispatch(
                &mut self,
                params: &vte::Params,
                _intermediates: &[u8],
                _private: bool,
                _final_byte: char,
            ) {
                for param in params {
                    for p in param {
                        p.hash(&mut self.hash);
                    }
                }
            }
            fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
                for param in params {
                    param.hash(&mut self.hash);
                }
            }
        }

        let mut parser = vte::Parser::new();
        let mut process = Process::default();
        for _ in 0..CORPUS_REPEAT {
            parser.advance(&mut process, &corpus);
        }

        divan::black_box_drop(parser);
    });
}

#[divan::bench]
fn vt_push_parser_parse_mixed(b: divan::Bencher) {
    let corpus = vt_push_parser::ascii::decode_string(MIXED);
    b.bench(move || {
        let mut parser = vt_push_parser::VTPushParser::new();
        #[derive(Default)]
        struct Process {
            hash: std::hash::DefaultHasher,
        }

        impl vt_push_parser::VTEventCallback for &'_ mut Process {
            fn event(&mut self, event: vt_push_parser::event::VTEvent) {
                event.hash(&mut self.hash);
            }
        }

        let mut process = Process::default();
        for _ in 0..CORPUS_REPEAT {
            parser.feed_with(&corpus, &mut process);
        }

        divan::black_box_drop(parser);
    });
}

#[divan::bench]
fn vt_push_parser_parse_mixed_sum(b: divan::Bencher) {
    let corpus = vt_push_parser::ascii::decode_string(MIXED);
    b.bench(move || {
        let mut parser = vt_push_parser::VTPushParser::new();
        #[derive(Default)]
        struct Process {
            hash: std::hash::DefaultHasher,
        }

        impl vt_push_parser::VTEventCallback for &'_ mut Process {
            fn event(&mut self, event: vt_push_parser::event::VTEvent) {
                use vt_push_parser::event::VTEvent::*;
                match event {
                    Raw(data) => {
                        data.hash(&mut self.hash);
                    }
                    Csi(b) => {
                        for param in b.params.numeric() {
                            for p in param {
                                p.hash(&mut self.hash);
                            }
                        }
                    }
                    OscStart => {
                        self.hash.write_u8(0);
                    }
                    OscData(data) => {
                        data.hash(&mut self.hash);
                    }
                    OscEnd { data, used_bel } => {
                        data.hash(&mut self.hash);
                    }
                    OscCancel => {
                        self.hash.write_u8(0);
                    }
                    _ => {}
                }
            }
        }

        let mut process = Process::default();
        for _ in 0..CORPUS_REPEAT {
            parser.feed_with(&corpus, &mut process);
        }

        divan::black_box_drop(parser);
    });
}

fn main() {
    divan::main();
}
