//! Measures the attach path with the real binaries: the time from a key to
//! its echo on the screen, without and with a flood of output in a second
//! session, and the memory of the daemon with one attached client.
//!
//! `tests/README.md` says how to run it. The numbers of a debug build have
//! no meaning.

use std::thread;
use std::time::{Duration, Instant};

use nomux_end_to_end::{
    Sandbox, TerminalClient, binary, key_to_echo, latency_row, resident_memory_kib,
};

/// The number of keys of one measurement.
const KEYS: usize = 2000;
/// The daemon is idle this long before its memory is read.
const IDLE_TIME: Duration = Duration::from_secs(2);

/// Writes 1.4 MB and then waits 0.09 seconds: 10 MB/s with the time of the
/// two programs.
const FLOOD_10_MB_PER_SECOND: &str =
    "while :; do head -c 1400000 /dev/zero | tr '\\0' 'x'; sleep 0.09; done";
/// Writes as fast as the holder reads.
const FLOOD_WITHOUT_LIMIT: &str = "yes 'a line of the flood of output'";

fn main() {
    let sandbox = Sandbox::new();
    // The terminal echoes each key of a command that reads lines.
    let mut typist = attach_to_new_session(&sandbox, "cat");
    typist.type_keys(b"ready\r");
    typist.wait_for_text("ready");

    println!("| Measurement | Median | 99th percentile | Maximum |");
    println!("|---|---|---|---|");
    latency_row(
        "Key to echo, no other output",
        &key_to_echo(&mut typist, None, KEYS),
    );

    thread::sleep(IDLE_TIME);
    let idle_memory = resident_memory_kib(sandbox.daemon_pid());

    for (name, command) in [
        (
            "Key to echo, 10 MB/s flood in a second session",
            FLOOD_10_MB_PER_SECOND,
        ),
        (
            "Key to echo, flood without a limit in a second session",
            FLOOD_WITHOUT_LIMIT,
        ),
    ] {
        let mut flood = attach_to_new_session(&sandbox, command);
        flood.wait_for_rows("the flood", |rows| !rows[0].is_empty());
        let started = Instant::now();
        let before = flood.discard_output();
        let latencies = key_to_echo(&mut typist, Some(&mut flood), KEYS);
        let megabytes = megabytes(flood.discard_output() - before);
        let rate = megabytes / started.elapsed().as_secs_f64();
        latency_row(
            &format!("{name} ({rate:.0} MB/s to its client)"),
            &latencies,
        );
    }

    println!();
    println!("| Measurement | Result |");
    println!("|---|---|");
    println!(
        "| Resident memory of the daemon, idle, 1 session with 1 attached client | {:.1} MB |",
        megabytes(idle_memory * 1024)
    );
    for name in ["nomuxd", "nomux", "nomux-holder"] {
        let size = binary(name)
            .metadata()
            .expect("the binary has a size")
            .len();
        println!("| Size of `{name}` | {size} bytes |");
    }
}

fn attach_to_new_session(sandbox: &Sandbox, command: &str) -> TerminalClient {
    let created = sandbox.nomux_ok(&["new", "--quiet", "--", "sh", "-c", command]);
    sandbox.nomux_in_terminal(&["attach", created.stdout.trim()])
}

#[expect(clippy::cast_precision_loss, reason = "a size for a report")]
fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000.0
}
