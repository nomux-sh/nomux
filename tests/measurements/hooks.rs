//! Measures `nomux hook`: the wall time of one hook event from the start of
//! the process to its end, with the test adapter installed and the daemon
//! running, and with the daemon stopped.
//!
//! ```sh
//! cargo build --release --workspace
//! cargo run --release -p nomux-end-to-end --example hook_measurements
//! ```

use std::fs;
use std::io::Write;
use std::process::Stdio;
use std::time::{Duration, Instant};

use nomux_client::{Client, ClientKind};
use nomux_end_to_end::{Sandbox, adapter_package, wait_for};

const RUNS: usize = 100;
const DATA: &[u8] = br#"{"tool":"read"}"#;

fn main() {
    let sandbox = Sandbox::new();
    let package = adapter_package(&sandbox.home().join("packages"));
    let package = package.display().to_string();
    let _ = sandbox.nomux_ok(&["ext", "install", &package, "--grant-all"]);
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh"])
        .stdout
        .trim()
        .to_owned();
    let token = token_of(&sandbox, &id);
    let socket = sandbox.daemon_socket().display().to_string();
    let variables = [
        ("NOMUX_SESSION_ID", id.as_str()),
        ("NOMUX_SOCK", socket.as_str()),
        ("NOMUX_TOKEN", token.as_str()),
    ];

    // The first event starts the adapter: the hook does not wait for it.
    let _ = hook(&sandbox, &variables);
    let running: Vec<Duration> = (0..RUNS).map(|_| hook(&sandbox, &variables)).collect();
    let _ = sandbox.nomux_ok(&["daemon", "stop"]);
    let stopped: Vec<Duration> = (0..RUNS).map(|_| hook(&sandbox, &variables)).collect();

    println!("| `nomux hook`, {RUNS} runs | Median | Fastest | Slowest |");
    println!("|---|---|---|---|");
    row("daemon runs, adapter installed", running);
    row("daemon stopped", stopped);
}

/// Runs one hook as a tool does: a new process with the data on its
/// standard input. Returns the time from its start to its end.
fn hook(sandbox: &Sandbox, variables: &[(&str, &str)]) -> Duration {
    let mut command = sandbox.nomux_command(&["hook", "testtool", "tool"]);
    command
        .envs(variables.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let start = Instant::now();
    let mut process = command.spawn().expect("the CLI starts");
    let mut stdin = process.stdin.take().expect("the input is piped");
    stdin.write_all(DATA).expect("the hook reads its input");
    drop(stdin);
    let status = process.wait().expect("the CLI ends");
    let time = start.elapsed();
    assert!(status.success(), "a hook does not fail");
    time
}

/// The token of the session `id`: its shell writes it to a file.
fn token_of(sandbox: &Sandbox, id: &str) -> String {
    let file = sandbox.home().join("token");
    let line = format!("printf %s \"$NOMUX_TOKEN\" > {}\n", file.display());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime can be made")
        .block_on(async {
            let mut client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
                .await
                .expect("the daemon answers");
            client
                .send_input(id, line.as_bytes())
                .await
                .expect("the session takes the input");
        });
    wait_for("the shell wrote the token", || {
        fs::read_to_string(&file)
            .ok()
            .filter(|token| token.len() == 32)
    })
}

fn row(name: &str, mut times: Vec<Duration>) {
    times.sort();
    let milliseconds = |time: Duration| time.as_secs_f64() * 1000.0;
    println!(
        "| {name} | {:.1} ms | {:.1} ms | {:.1} ms |",
        milliseconds(times[RUNS / 2]),
        milliseconds(times[0]),
        milliseconds(times[RUNS - 1]),
    );
}
