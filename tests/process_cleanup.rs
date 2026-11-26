//! The test support leaves no process behind: a sandbox stops the
//! processes that its test left, and the test fails for a process that the
//! normal stop did not reach.

use std::os::unix::process::CommandExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::{Command, Stdio};

use nomux_end_to_end::{ProcessGuard, Sandbox, process_runs, wait_for};

#[test]
fn a_process_that_a_test_left_is_stopped_and_the_test_fails() {
    let sandbox = Sandbox::new();
    // A process that no daemon and no holder knows, in a session of its
    // own, as a daemon that waits for the lock of a different daemon is.
    let mut left = Command::new("sleep")
        .arg("300")
        .env("HOME", sandbox.home())
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();

    let dropped = catch_unwind(AssertUnwindSafe(|| drop(sandbox)));

    // The exit status is collected, thus the process is not a zombie.
    let _ = left.wait();
    assert!(!process_runs(left.id()));
    let panic = dropped.expect_err("a sandbox with a process that was left does not fail");
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(
        message.contains("the test left processes behind"),
        "{message}"
    );
    assert!(
        message.contains(&format!("process {}: sleep 300", left.id())),
        "{message}"
    );
}

#[test]
fn a_sandbox_stops_the_sessions_and_the_daemon_of_its_test_without_a_failure() {
    let sandbox = Sandbox::new();
    let created = sandbox
        .nomux_ok(&["new", "--json", "--", "sh", "-c", "sleep 300 & sleep 300"])
        .json();
    let command = u32::try_from(created["session"]["pid"].as_u64().unwrap()).unwrap();
    let id = created["session"]["id"].as_str().unwrap();
    let holder = sandbox.holder_pid(id);
    let daemon = sandbox.daemon_pid();

    drop(sandbox);

    for pid in [command, holder, daemon] {
        assert!(!process_runs(pid), "process {pid} runs");
    }
}

#[test]
fn a_guarded_process_and_the_processes_of_its_group_end_with_the_guard() {
    let directory = tempfile::tempdir().unwrap();
    let pid_file = directory.path().join("child");
    let script = format!("sleep 300 & echo $! > {}; wait", pid_file.display());
    let mut guard = ProcessGuard::spawn(Command::new("sh").args(["-c", &script]));
    let leader = guard.process().id();
    let child: u32 = wait_for("the script started its child", || {
        std::fs::read_to_string(&pid_file).ok()?.trim().parse().ok()
    });

    drop(guard);

    assert!(!process_runs(leader));
    wait_for("the child of the script is gone", || {
        (!process_runs(child)).then_some(())
    });
}
