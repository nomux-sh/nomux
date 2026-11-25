//! The scan of a runtime directory: live holders and stale directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use nomux_holder_protocol::discovery::{
    DiscoveredHolder, HolderState, PID_FILE, ProcessIdentity, READY_FILE, StaleReason, scan,
};

fn holder_directory(runtime: &Path, name: &str, process: ProcessIdentity) -> PathBuf {
    let directory = runtime.join(name);
    fs::create_dir(&directory).expect("the test helper works");
    fs::write(directory.join(PID_FILE), format!("{process}\n")).expect("the test helper works");
    directory
}

fn scan_one(runtime: &Path) -> DiscoveredHolder {
    let mut holders = scan(runtime).expect("the test helper works");
    assert_eq!(holders.len(), 1, "{holders:?}");
    holders.remove(0)
}

/// The identity of a process that ran and ended.
fn ended_process() -> ProcessIdentity {
    let mut child = Command::new("true").spawn().expect("the test helper works");
    let identity = ProcessIdentity::of(child.id()).expect("the test helper works");
    child.wait().expect("the test helper works");
    identity.unwrap_or(ProcessIdentity {
        pid: child.id(),
        start_time: 0,
    })
}

#[test]
fn a_running_process_with_the_recorded_start_time_is_live_after_ready() {
    let runtime = tempfile::tempdir().unwrap();
    let this_process = ProcessIdentity::current().unwrap();
    let directory = holder_directory(runtime.path(), "session-1", this_process);

    let holder = scan_one(runtime.path());
    assert_eq!(holder.state, HolderState::Starting);

    fs::write(directory.join(READY_FILE), "").unwrap();
    let holder = scan_one(runtime.path());
    assert_eq!(holder.state, HolderState::Live);
    assert_eq!(holder.session_id.as_str(), "session-1");
    assert_eq!(holder.socket_path(), directory.join("sock"));
}

#[test]
fn a_directory_of_a_process_that_ended_is_stale_and_can_be_removed() {
    let runtime = tempfile::tempdir().unwrap();
    let directory = holder_directory(runtime.path(), "gone", ended_process());
    fs::write(directory.join(READY_FILE), "").unwrap();

    let holder = scan_one(runtime.path());
    assert_eq!(holder.state, HolderState::Stale(StaleReason::ProcessGone));

    holder.remove_directory().unwrap();
    assert_eq!(scan(runtime.path()).unwrap(), []);
}

#[test]
fn a_reused_process_id_with_a_different_start_time_is_stale() {
    let runtime = tempfile::tempdir().unwrap();
    let this_process = ProcessIdentity::current().unwrap();
    let earlier_process = ProcessIdentity {
        start_time: this_process.start_time - 1,
        ..this_process
    };
    let directory = holder_directory(runtime.path(), "reused", earlier_process);
    fs::write(directory.join(READY_FILE), "").unwrap();

    let holder = scan_one(runtime.path());
    assert_eq!(
        holder.state,
        HolderState::Stale(StaleReason::ProcessReplaced)
    );
}

#[test]
fn a_directory_without_a_valid_pid_file_is_stale() {
    let runtime = tempfile::tempdir().unwrap();
    let directory = runtime.path().join("broken");
    fs::create_dir(&directory).unwrap();
    assert_eq!(
        scan_one(runtime.path()).state,
        HolderState::Stale(StaleReason::InvalidPidFile)
    );

    fs::write(directory.join(PID_FILE), "not a process\n").unwrap();
    assert_eq!(
        scan_one(runtime.path()).state,
        HolderState::Stale(StaleReason::InvalidPidFile)
    );
}

#[test]
fn entries_that_are_not_holder_directories_are_ignored() {
    let runtime = tempfile::tempdir().unwrap();
    fs::write(runtime.path().join("file"), "").unwrap();
    fs::create_dir(runtime.path().join("not.a.session")).unwrap();

    assert_eq!(scan(runtime.path()).unwrap(), []);
    assert_eq!(scan(&runtime.path().join("missing")).unwrap(), []);
}
