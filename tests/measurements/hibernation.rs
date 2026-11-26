//! Measures the hibernation of agent sessions with the test adapter: the
//! memory of the host before and after the hibernation of agents that each
//! hold 200 MiB, the time from an attach to the first screen of the old run
//! and to the resumed run, and the CPU time and the context switches of the
//! idle daemon with each trigger on.
//!
//! ```sh
//! cargo build --release --workspace
//! cargo run --release -p nomux-end-to-end --example hibernation_measurements -- 3
//! ```
//!
//! The argument is the number of agent sessions. The available memory of a
//! host changes with each other program. Thus the program also reports the
//! memory of its own control group, which has each process that it starts:
//! run it in a scope of its own for that value.
//!
//! ```sh
//! systemd-run --user --scope --quiet -- target/release/examples/hibernation_measurements 3
//! ```

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::{Duration, Instant};

use nomux_end_to_end::{Sandbox, adapter_package, binary, wait_for};

const MEBIBYTE: u64 = 1024 * 1024;
const IDLE_TIME: Duration = Duration::from_mins(1);
/// The adapter extension stops 60 s after its last event, and the thread of
/// a memory measurement ends 10 s after it: the idle time starts later.
const SETTLE_TIME: Duration = Duration::from_secs(75);

fn main() {
    let agents: usize = std::env::args()
        .nth(1)
        .map_or(3, |count| count.parse().expect("a number of sessions"));
    let mut sandbox = Sandbox::new();
    // Each trigger is on, and none applies: the count is off only because
    // the program hibernates the sessions itself.
    sandbox.write_configuration(
        "[agents]\nmax_live = 0\nhibernate_on_pressure = true\n\
         [sessions]\ntotal_memory_limit = \"64G\"\n",
    );
    install_tool(&mut sandbox);
    let package = adapter_package(&sandbox.home().join("packages"));
    let _ = sandbox.nomux_ok(&["ext", "install", package.to_str().unwrap(), "--grant-all"]);

    let ids: Vec<String> = (0..agents)
        .map(|number| start_agent(&sandbox, &format!("agent{number}")))
        .collect();
    let measured = sandbox.nomux_ok(&["ls", "--memory", "--json"]).json();
    let used = measured["memory_total"]["command_bytes"].as_u64().unwrap();
    let available_before = available_memory();
    let group_before = control_group_memory();
    for id in &ids {
        let _ = sandbox.nomux_ok(&["hibernate", id]);
    }
    // The kernel gives the pages of the ended processes back at once.
    thread::sleep(Duration::from_secs(1));
    let available_after = available_memory();
    let group_after = control_group_memory();
    let measured = sandbox.nomux_ok(&["ls", "--memory", "--json"]).json();
    let freed = measured["memory_total"]["freed_bytes"].as_u64().unwrap();

    let daemon = sandbox.daemon_pid();
    thread::sleep(SETTLE_TIME);
    let before = DaemonCounters::of(daemon);
    thread::sleep(IDLE_TIME);
    let after = DaemonCounters::of(daemon);

    println!("{agents} agent sessions, each with 200 MiB");
    println!("| Measurement | Result |");
    println!("|---|---|");
    println!(
        "| Memory of the commands before the hibernation (`ls --memory`, pss) | {:.1} MiB |",
        mebibytes(used)
    );
    println!(
        "| Available memory of the host (`MemAvailable`): before, after, difference | {:.1} MiB, {:.1} MiB, {:.1} MiB |",
        mebibytes(available_before),
        mebibytes(available_after),
        mebibytes(available_after.saturating_sub(available_before))
    );
    if let (Some(before), Some(after)) = (group_before, group_after) {
        println!(
            "| Memory of the control group of the measurement (`memory.current`): before, after, difference | {:.1} MiB, {:.1} MiB, {:.1} MiB |",
            mebibytes(before),
            mebibytes(after),
            mebibytes(before.saturating_sub(after))
        );
    }
    println!(
        "| Memory that the hibernations gave back (`ls --memory` total) | {:.1} MiB |",
        mebibytes(freed)
    );
    println!(
        "| Idle daemon with each trigger on, {} s: CPU time, context switches, threads | {} clock ticks, {}, {} |",
        IDLE_TIME.as_secs(),
        after.cpu_ticks - before.cpu_ticks,
        after.context_switches - before.context_switches,
        after.threads
    );

    for (number, id) in ids.iter().enumerate() {
        let (screen, resumed) = wake(&sandbox, id, &format!("agent{number}"));
        println!(
            "| Wake {number}: attach to the first screen of the old run, to the resumed run | {:.1} ms, {:.1} ms |",
            milliseconds(screen),
            milliseconds(resumed)
        );
    }
}

/// Writes the agent tool `testtool`: it reports its work through the
/// hooks of the test adapter, and then holds 200 MiB.
fn install_tool(sandbox: &mut Sandbox) {
    let bin = sandbox.home().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = --resume ]; then name=$2; echo "testtool resumed $name"; else name=$1; echo "testtool started $name"; fi
sleep 0.5
printf '{{"session":"%s"}}' "$name" | {nomux} hook testtool prompt
printf '{{}}' | {nomux} hook testtool stop
exec {memory_user} "$HOME/$name.ready" 200
"#,
        nomux = binary("nomux").display(),
        memory_user = binary("nomux-test-memory-user").display()
    );
    let tool = bin.join("testtool");
    fs::write(&tool, script).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    sandbox.set_variable("PATH", format!("{}:/usr/bin:/bin", bin.display()));
}

fn start_agent(sandbox: &Sandbox, name: &str) -> String {
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "testtool", name])
        .stdout
        .trim()
        .to_owned();
    let ready = sandbox.home().join(format!("{name}.ready"));
    wait_for("the agent holds its memory", || {
        ready.exists().then_some(())
    });
    wait_for("the agent reports that it is done", || {
        let explained = sandbox
            .nomux_ok(&["agent", "explain", &id, "--json"])
            .json();
        (explained["explain"]["run"]["status"]["state"] == "done").then_some(())
    });
    fs::remove_file(ready).unwrap();
    id
}

/// Attaches to the hibernated session `id`. Returns the time to the first
/// screen of the old run and the time to the first output of the resumed
/// run.
fn wake(sandbox: &Sandbox, id: &str, name: &str) -> (Duration, Duration) {
    let start = Instant::now();
    let mut terminal = sandbox.nomux_in_window(&["attach", id], 30, 100);
    while !terminal
        .text_output()
        .contains(&format!("testtool started {name}"))
    {
        terminal.wait_for_output();
    }
    let screen = start.elapsed();
    while !terminal
        .text_output()
        .contains(&format!("testtool resumed {name}"))
    {
        terminal.wait_for_output();
    }
    let resumed = start.elapsed();
    terminal.type_keys(b"\x1cd");
    terminal.wait_for_exit();
    (screen, resumed)
}

/// `MemAvailable` of `/proc/meminfo` in bytes.
fn available_memory() -> u64 {
    let meminfo = fs::read_to_string("/proc/meminfo").expect("Linux has /proc/meminfo");
    let line = meminfo
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))
        .expect("the kernel gives MemAvailable");
    let kibibytes: u64 = line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .expect("a number of kB");
    kibibytes * 1024
}

/// `memory.current` of the control group of this process in bytes: the
/// memory of each process that the measurement started. `None` without a
/// control group of version 2.
fn control_group_memory() -> Option<u64> {
    let membership = fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))?;
    fs::read_to_string(format!("/sys/fs/cgroup{path}/memory.current"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The CPU time and the context switches of each thread of a process.
struct DaemonCounters {
    cpu_ticks: u64,
    context_switches: u64,
    threads: usize,
}

impl DaemonCounters {
    fn of(pid: u32) -> Self {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).expect("the daemon runs");
        // The fields after the name of the program, which is in brackets.
        let fields: Vec<&str> = stat[stat.rfind(')').unwrap() + 2..]
            .split_whitespace()
            .collect();
        let cpu_ticks = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
        let tasks: Vec<_> = fs::read_dir(format!("/proc/{pid}/task"))
            .expect("the daemon runs")
            .filter_map(Result::ok)
            .collect();
        let context_switches = tasks
            .iter()
            .map(|task| {
                let status = fs::read_to_string(task.path().join("status")).unwrap_or_default();
                status
                    .lines()
                    .filter(|line| line.contains("ctxt_switches:"))
                    .filter_map(|line| line.split_whitespace().nth(1)?.parse::<u64>().ok())
                    .sum::<u64>()
            })
            .sum();
        Self {
            cpu_ticks,
            context_switches,
            threads: tasks.len(),
        }
    }
}

#[expect(clippy::cast_precision_loss, reason = "a value for a person")]
fn mebibytes(bytes: u64) -> f64 {
    bytes as f64 / MEBIBYTE as f64
}

fn milliseconds(time: Duration) -> f64 {
    time.as_secs_f64() * 1000.0
}
