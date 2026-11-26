//! Measures the start of the daemon after a restart of the host: the time
//! from the start of `nomuxd` to a complete list of the dormant sessions.
//!
//! ```sh
//! cargo build --release --workspace
//! cargo run --release -p nomux-end-to-end --example restore_measurements -- 50
//! ```
//!
//! The argument is the number of sessions. Each session gets a journal of
//! the size limit: 32 MiB of files. The program needs that space in `/tmp`.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use nomux_end_to_end::{Sandbox, process_runs, wait_for};
use nomux_holder_protocol::session_state::JOURNAL_DIRECTORY;

const RUNS: usize = 11;
/// The holder keeps the journal of a session below 32 MiB.
const FULL_JOURNAL_BYTES: u64 = 30 * 1024 * 1024;
/// Text that LZ4 does not make smaller: more than the size limit of the
/// journal.
const FILL_JOURNAL: &str = "head -c 30000000 /dev/urandom | base64; sleep 100000";

fn journal_bytes(journal: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(journal) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok()?.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn main() {
    let sessions: usize = std::env::args()
        .nth(1)
        .map_or(50, |count| count.parse().expect("a number of sessions"));
    let sandbox = Sandbox::new();

    let mut journals = Vec::new();
    for _ in 0..sessions {
        let created = sandbox
            .nomux_ok(&["new", "--quiet", "--", "sh", "-c", FILL_JOURNAL])
            .stdout;
        let journal = sandbox
            .session_state_directory(created.trim())
            .join(JOURNAL_DIRECTORY);
        wait_for("the journal is full", || {
            (journal_bytes(&journal) >= FULL_JOURNAL_BYTES).then_some(())
        });
        journals.push(journal);
    }
    let total: u64 = journals.iter().map(|journal| journal_bytes(journal)).sum();
    sandbox.kill_each_process();

    let mut times = Vec::new();
    for _ in 0..RUNS {
        // The CLI starts the daemon, waits for its socket and asks for the
        // list. The time has the three parts.
        let start = Instant::now();
        let listed = sandbox.sessions();
        times.push(start.elapsed());
        assert_eq!(listed.len(), sessions);
        assert!(listed.iter().all(|session| session["state"] == "dormant"));
        assert!(sandbox.holders().is_empty(), "a holder started");

        let daemon = sandbox.daemon_pid();
        let _ = sandbox.nomux_ok(&["daemon", "stop"]);
        wait_for("the daemon is gone", || {
            (!process_runs(daemon)).then_some(())
        });
    }
    times.sort();
    let milliseconds = |time: Duration| time.as_secs_f64() * 1000.0;
    println!(
        "{sessions} dormant sessions, {} MiB of journal files",
        total / (1024 * 1024)
    );
    println!(
        "daemon start to a complete `nomux ls --json`: median {:.1} ms, fastest {:.1} ms, slowest {:.1} ms ({RUNS} runs)",
        milliseconds(times[RUNS / 2]),
        milliseconds(times[0]),
        milliseconds(times[RUNS - 1]),
    );
}
