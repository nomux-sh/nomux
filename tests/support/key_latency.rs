//! The measurement of the time from a key to its echo on the screen of
//! `nomux attach`. The measurement programs use it.

use std::thread;
use std::time::{Duration, Instant};

use crate::TerminalClient;

/// A person types a key each few milliseconds at most.
const PAUSE_BETWEEN_KEYS: Duration = Duration::from_millis(2);

/// Types `keys` keys, one at a time, and measures the time until the
/// terminal of the client has the echo of each key. During that, takes the
/// output of `flood` away. Returns the times in their sequence from the
/// shortest to the longest.
///
/// # Panics
///
/// Panics if an echo does not come in 20 seconds.
pub fn key_to_echo(
    typist: &mut TerminalClient,
    mut flood: Option<&mut TerminalClient>,
    keys: usize,
) -> Vec<Duration> {
    let mut latencies = Vec::with_capacity(keys);
    for _ in 0..keys {
        // Output from before the key is not the echo of the key.
        typist.read_pending_output();
        let typed = Instant::now();
        typist.type_keys(b"k");
        typist.wait_for_output();
        latencies.push(typed.elapsed());
        if let Some(flood) = flood.as_deref_mut() {
            flood.discard_output();
        }
        thread::sleep(PAUSE_BETWEEN_KEYS);
    }
    latencies.sort_unstable();
    latencies
}

/// Prints one row of a Markdown table: the name, the median, the 99th
/// percentile and the maximum of `sorted`.
///
/// # Panics
///
/// Panics if `sorted` is empty.
pub fn latency_row(name: &str, sorted: &[Duration]) {
    let at = |part: usize| sorted[(sorted.len() - 1) * part / 100];
    let milliseconds = |duration: Duration| duration.as_secs_f64() * 1000.0;
    println!(
        "| {name} | {:.2} ms | {:.2} ms | {:.2} ms |",
        milliseconds(at(50)),
        milliseconds(at(99)),
        milliseconds(at(100)),
    );
}
