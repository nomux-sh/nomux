//! Measures the start of a new session with the real binaries: the time of
//! `nomux new`, the time to the first command that the shell of the session
//! runs, where the time goes without the pool, and the memory and the CPU
//! time of the daemon and the pooled holders.
//!
//! ```sh
//! cargo build --release --workspace
//! cargo run --release -p nomux-end-to-end --example new_session_measurements
//! ```
//!
//! The numbers of a debug build have no meaning. The memory and the CPU time
//! come from `/proc`, thus that part needs Linux.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_client::{AttachOptions, Client, ClientKind, NewSession, TerminalEvent};
use nomux_end_to_end::{Sandbox, binary, resident_memory_kib, wait_for};
use nomux_holder_protocol::discovery::{
    HolderState, PID_FILE, ProcessIdentity, READY_FILE, scan_pooled,
};

const RUNS: usize = 50;
/// The pool of the default configuration.
const POOL_SIZE: usize = 2;
/// The time in which the CPU time of the idle processes is read.
const IDLE_TIME: Duration = Duration::from_secs(30);
/// A shell whose start takes as long as a shell with many start files.
const SLOW_START_FILE: &str = "sleep 0.2\n";
/// The shell runs this line when it reads input. The quotes keep the echo
/// of the typed line from the text that the test waits for.
const FIRST_COMMAND: &[u8] = b"echo RE''ADY\r";
/// Longer than the start of the slow shell: a person starts the next
/// session later than that.
const SHELL_START: Duration = Duration::from_millis(300);

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");

    for (name, configuration, slow_shell) in [
        ("no pool", "[pool]\nsize = 0\n", false),
        ("warm pool", "", false),
        ("no pool, shell start of 200 ms", "[pool]\nsize = 0\n", true),
        ("warm pool, shell start of 200 ms", "", true),
    ] {
        let sandbox = sandbox(configuration, slow_shell);
        let project = project_directory(&sandbox);
        let pooled = !configuration.contains("size = 0");
        if !slow_shell {
            memory_and_idle_cpu(&sandbox, name, pooled);
        }
        println!("| Measurement | Median | Slowest of {RUNS} |");
        println!("|---|---|---|");
        row(
            &format!("`nomux new`, {name}"),
            &cli_new(&sandbox, &project, pooled),
        );
        let first_command = runtime.block_on(first_command(&sandbox, &project, pooled));
        row(
            &format!("Request to the first command, {name}"),
            &first_command,
        );
        if !pooled && !slow_shell {
            breakdown(&sandbox, &project, &runtime);
        }
        println!();
    }
}

fn sandbox(configuration: &str, slow_shell: bool) -> Sandbox {
    let mut sandbox = Sandbox::new();
    sandbox.write_configuration(configuration);
    if slow_shell {
        fs::write(sandbox.home().join(".bashrc"), SLOW_START_FILE).expect("a start file");
        sandbox.set_variable("SHELL", "/bin/bash");
    }
    sandbox
}

/// A working directory that is not the home directory: a session from the
/// pool changes to it.
fn project_directory(sandbox: &Sandbox) -> PathBuf {
    let project = sandbox.home().join("project");
    fs::create_dir_all(&project).expect("the directory can be made");
    project
}

/// The time of `nomux new` from the start of the CLI to its end.
fn cli_new(sandbox: &Sandbox, project: &Path, pooled: bool) -> Vec<Duration> {
    let _ = sandbox.sessions();
    let mut times = Vec::new();
    for _ in 0..RUNS {
        if pooled {
            wait_for_full_pool(sandbox);
        }
        let mut command = sandbox.nomux_command(&["new", "--quiet"]);
        command.current_dir(project).stdout(Stdio::null());
        let start = Instant::now();
        let status = command.status().expect("the CLI starts");
        times.push(start.elapsed());
        assert!(status.success());
        // Each run starts with the same sessions: none.
        kill_sessions(sandbox);
    }
    times
}

/// The time from the request of a client that is connected to the output of
/// the first command that the shell of the new session runs: the session
/// accepts input and its shell is in the working directory.
async fn first_command(sandbox: &Sandbox, project: &Path, pooled: bool) -> Vec<Duration> {
    let mut times = Vec::new();
    for _ in 0..RUNS {
        if pooled {
            wait_for_full_pool(sandbox);
        }
        let mut client = connect(sandbox).await;
        let start = Instant::now();
        let session = client
            .create_session(NewSession {
                command: Vec::new(),
                working_directory: Some(project.to_owned()),
            })
            .await
            .expect("the session starts");
        let attacher = connect(sandbox).await;
        let (mut output, mut input) = attacher
            .attach(&session.id, AttachOptions::default())
            .await
            .expect("the attach");
        input.send(FIRST_COMMAND).await.expect("the input");
        let mut text = Vec::new();
        while !contains(&text, b"READY") {
            match output.next().await.expect("the output") {
                TerminalEvent::Output(bytes) | TerminalEvent::Snapshot { bytes, .. } => {
                    text.extend_from_slice(bytes);
                }
                _ => {}
            }
        }
        times.push(start.elapsed());
        drop((output, input));
        kill_sessions(sandbox);
    }
    times
}

/// Where the time of a session without the pool goes.
fn breakdown(sandbox: &Sandbox, project: &Path, runtime: &tokio::runtime::Runtime) {
    let cli = repeat(|| {
        let start = Instant::now();
        let _ = sandbox.nomux_ok(&["ls"]);
        start.elapsed()
    });
    row(
        "Breakdown: `nomux ls` (start of the CLI, connection, one request)",
        &cli,
    );
    let create = runtime.block_on(async {
        let mut client = connect(sandbox).await;
        let mut times = Vec::new();
        for _ in 0..RUNS {
            let start = Instant::now();
            let session = client
                .create_session(NewSession {
                    command: Vec::new(),
                    working_directory: Some(project.to_owned()),
                })
                .await
                .expect("the session starts");
            times.push(start.elapsed());
            client
                .kill_session(&session.id)
                .await
                .expect("the session ends");
        }
        times
    });
    row(
        "Breakdown: `Session.Create` of a connected client (start of the holder, wait for `ready`, lease)",
        &create,
    );
    kill_sessions(sandbox);
    let mut run = 0;
    let holder = repeat(|| {
        run += 1;
        holder_to_ready(sandbox, run)
    });
    row(
        "Breakdown: start of `nomux-holder` to its `ready` file, read each 0.1 ms",
        &holder,
    );
}

/// Starts a holder without the daemon and waits for its `ready` file.
fn holder_to_ready(sandbox: &Sandbox, run: usize) -> Duration {
    let runtime_directory = sandbox.home().join("holders");
    let session = format!("measurement{run}");
    let start = Instant::now();
    let mut holder = Command::new(binary("nomux-holder"))
        .args([
            "--session",
            &session,
            "--exit-timeout",
            "0",
            "--runtime-directory",
        ])
        .arg(&runtime_directory)
        .arg("--state-directory")
        .arg(sandbox.home().join("holder-state"))
        .args(["--", "/bin/sh", "-c", "sleep 2"])
        .env("HOME", sandbox.home())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the holder starts");
    let ready = runtime_directory.join(&session).join(READY_FILE);
    while !ready.exists() {
        thread::sleep(Duration::from_micros(100));
    }
    let time = start.elapsed();
    let _ = holder.kill();
    let _ = holder.wait();
    time
}

/// The memory of a new daemon without a session, and of the pooled holders
/// with their shells, and their CPU time while they are idle.
fn memory_and_idle_cpu(sandbox: &Sandbox, name: &str, pooled: bool) {
    let _ = sandbox.sessions();
    if pooled {
        wait_for_full_pool(sandbox);
    }
    thread::sleep(Duration::from_secs(5));
    let daemon = sandbox.daemon_pid();
    let holders = if pooled {
        pooled_holder_pids(sandbox)
    } else {
        Vec::new()
    };
    let shells: Vec<u32> = holders.iter().flat_map(|pid| children(*pid)).collect();
    let megabytes = |pids: &[u32]| {
        let kibibytes: u64 = pids.iter().map(|pid| resident_memory_kib(*pid)).sum();
        f64::from(u32::try_from(kibibytes).expect("less than 4 TiB")) / 1024.0
    };
    println!("| Measurement, {name} | Result |");
    println!("|---|---|");
    println!(
        "| Resident memory of the daemon without a session | {:.1} MB |",
        megabytes(&[daemon])
    );
    if pooled {
        println!(
            "| Resident memory of the {POOL_SIZE} pooled holders | {:.1} MB |",
            megabytes(&holders)
        );
        println!(
            "| Resident memory of their {} shells | {:.1} MB |",
            shells.len(),
            megabytes(&shells)
        );
    }
    let processes: Vec<u32> = [daemon].into_iter().chain(holders).chain(shells).collect();
    let before: Vec<u64> = processes.iter().map(|pid| cpu_ticks(*pid)).collect();
    thread::sleep(IDLE_TIME);
    let after: Vec<u64> = processes.iter().map(|pid| cpu_ticks(*pid)).collect();
    let used: u64 = after
        .iter()
        .zip(&before)
        .map(|(after, before)| after - before)
        .sum();
    println!(
        "| CPU time of the daemon, the pooled holders and their shells in {} s, idle | {used} clock ticks |",
        IDLE_TIME.as_secs()
    );
    println!();
}

fn wait_for_full_pool(sandbox: &Sandbox) {
    wait_for("the pool is full", || {
        let live = scan_pooled(&sandbox.runtime_directory())
            .expect("the runtime directory can be read")
            .into_iter()
            .filter(|holder| holder.state == HolderState::Live)
            .count();
        (live >= POOL_SIZE).then_some(())
    });
    // The daemon has the lease of a pooled holder a moment after its
    // `ready` file, and the shell needs the time of its start files.
    thread::sleep(SHELL_START);
}

fn pooled_holder_pids(sandbox: &Sandbox) -> Vec<u32> {
    scan_pooled(&sandbox.runtime_directory())
        .expect("the runtime directory can be read")
        .into_iter()
        .map(|holder| {
            let identity: ProcessIdentity = fs::read_to_string(holder.directory.join(PID_FILE))
                .expect("a pid file")
                .parse()
                .expect("a valid pid file");
            identity.pid
        })
        .collect()
}

fn children(pid: u32) -> Vec<u32> {
    fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|child| child.parse().ok())
        .collect()
}

/// User and system CPU time in clock ticks: fields 14 and 15 of `stat`.
fn cpu_ticks(pid: u32) -> u64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("the process runs");
    let fields: Vec<&str> = stat[stat.rfind(')').expect("a stat line") + 2..]
        .split(' ')
        .collect();
    fields[11].parse::<u64>().expect("utime") + fields[12].parse::<u64>().expect("stime")
}

fn kill_sessions(sandbox: &Sandbox) {
    for session in sandbox.sessions() {
        let id = session["id"].as_str().expect("an identifier");
        let _ = sandbox.nomux(&["kill", id]);
    }
}

async fn connect(sandbox: &Sandbox) -> Client {
    Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
        .await
        .expect("the daemon answers")
}

fn repeat(mut measure: impl FnMut() -> Duration) -> Vec<Duration> {
    (0..RUNS).map(|_| measure()).collect()
}

fn contains(text: &[u8], part: &[u8]) -> bool {
    text.windows(part.len()).any(|window| window == part)
}

fn row(name: &str, times: &[Duration]) {
    let mut sorted = times.to_vec();
    sorted.sort();
    let milliseconds = |time: Duration| time.as_secs_f64() * 1000.0;
    println!(
        "| {name} | {:.2} ms | {:.2} ms |",
        milliseconds(sorted[sorted.len() / 2]),
        milliseconds(sorted[sorted.len() - 1])
    );
}
