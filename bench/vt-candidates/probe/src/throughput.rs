//! Parse throughput of a candidate on one corpus.

use std::time::{Duration, Instant};

use crate::candidate::{Parser, Size};
use crate::corpus::Corpus;

/// The size of a common terminal window.
const SIZE: Size = Size {
    rows: 24,
    columns: 80,
};
const SCROLLBACK_LINES: usize = 1000;
/// The holder reads the PTY in blocks of this size.
const READ_SIZE: usize = 64 * 1024;

/// Feeds `bytes` bytes of `corpus` to a new terminal `runs` times.
/// Prints the corpus size in bytes and the median time in nanoseconds.
pub(crate) fn report<P: Parser>(corpus: Corpus, bytes: usize, runs: usize) {
    let input = corpus.generate(bytes);
    let mut times: Vec<Duration> = (0..runs).map(|_| time_one_run::<P>(&input)).collect();
    times.sort_unstable();
    println!("{} {}", input.len(), times[times.len() / 2].as_nanos());
}

fn time_one_run<P: Parser>(input: &[u8]) -> Duration {
    let mut parser = P::new(SIZE, SCROLLBACK_LINES);
    let start = Instant::now();
    for block in input.chunks(READ_SIZE) {
        parser.feed(block);
    }
    start.elapsed()
}
