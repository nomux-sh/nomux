//! Benchmark probe without a VT library.
//!
//! The size and the memory of this probe are the reference: the difference
//! between another probe and this probe is the cost of the candidate.

use std::hint::black_box;
use std::process::ExitCode;

use vt_probe::{Parser, Size};

struct Baseline {
    bytes: usize,
}

impl Parser for Baseline {
    const NAME: &'static str = "baseline (no VT library)";

    fn new(_size: Size, _scrollback_lines: usize) -> Self {
        Self { bytes: 0 }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.bytes += black_box(bytes).len();
    }

    fn retained_scrollback_lines(&self) -> usize {
        0
    }
}

fn main() -> ExitCode {
    vt_probe::run_parser::<Baseline>()
}
