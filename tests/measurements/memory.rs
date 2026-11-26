//! Measures the request for the memory of each session: the time of one
//! `Session.Memory` request with many sessions, and the time of the
//! `nomux ls --memory` command.
//!
//! ```sh
//! cargo build --release --workspace
//! cargo run --release -p nomux-end-to-end --example memory_measurements -- 50
//! ```
//!
//! The argument is the number of sessions. The command of each session is
//! a shell with two children: three processes for each session.

use std::time::{Duration, Instant};

use nomux_client::{Client, ClientKind};
use nomux_end_to_end::Sandbox;

const RUNS: usize = 21;
const COMMAND: &str = "sleep 100000 & sleep 100000 & wait";
const PROCESSES_OF_COMMAND: u32 = 3;

fn main() {
    let sessions: usize = std::env::args()
        .nth(1)
        .map_or(50, |count| count.parse().expect("a number of sessions"));
    let sandbox = Sandbox::new();
    for _ in 0..sessions {
        let _ = sandbox.nomux_ok(&["new", "--quiet", "--", "sh", "-c", COMMAND]);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime can be made");
    let (requests, processes) = runtime.block_on(async {
        let mut client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
            .await
            .expect("the daemon answers");
        let mut times = Vec::new();
        let mut processes = 0;
        for _ in 0..RUNS {
            let start = Instant::now();
            let measured = client.session_memory().await.expect("the request succeeds");
            times.push(start.elapsed());
            assert_eq!(measured.len(), sessions);
            processes = measured
                .iter()
                .map(|session| session.command_processes)
                .sum::<u32>();
        }
        (times, processes)
    });
    let expected = u32::try_from(sessions).expect("a small number") * PROCESSES_OF_COMMAND;
    assert_eq!(processes, expected, "each process of each command counts");

    let commands = |arguments: &[&str]| -> Vec<Duration> {
        (0..RUNS)
            .map(|_| {
                let start = Instant::now();
                let _ = sandbox.nomux_ok(arguments);
                start.elapsed()
            })
            .collect()
    };
    let with_memory = commands(&["ls", "--memory", "--json"]);
    let without_memory = commands(&["ls", "--json"]);

    let host_processes = std::fs::read_dir("/proc").map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().parse::<u32>().is_ok())
            .count()
    });
    println!(
        "{sessions} sessions with {processes} processes of their commands; {host_processes} \
         processes on the host (0: no /proc)"
    );
    println!("| Measurement | Median | Fastest | Slowest |");
    println!("|---|---|---|---|");
    row("`Session.Memory` request", requests);
    row("`nomux ls --memory --json` command", with_memory);
    row("`nomux ls --json` command", without_memory);
}

fn row(name: &str, mut times: Vec<Duration>) {
    times.sort();
    let milliseconds = |time: Duration| time.as_secs_f64() * 1000.0;
    println!(
        "| {name}, {RUNS} runs | {:.1} ms | {:.1} ms | {:.1} ms |",
        milliseconds(times[RUNS / 2]),
        milliseconds(times[0]),
        milliseconds(times[RUNS - 1]),
    );
}
