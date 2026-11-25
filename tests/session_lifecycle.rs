//! The life of a session with the real binaries: the CLI starts the daemon,
//! the daemon starts a holder, the daemon ends in different ways, and the
//! session continues.

use std::fs;

use nomux_end_to_end::{Sandbox, kill_process, kill_process_group, process_runs, wait_for};
use nomux_holder_protocol::discovery::{HolderState, PID_FILE};
use serde_json::Value;

const NO_SESSIONS: [Value; 0] = [];

/// Starts a session with a shell and returns its identifier and the process
/// ID of the shell.
fn new_session(sandbox: &Sandbox) -> (String, u32) {
    let created = sandbox.nomux_ok(&["new", "--json", "--cmd", "sh"]).json();
    assert_eq!(created["schema_version"], 1);
    let session = &created["session"];
    assert_eq!(session["state"], "running");
    let id = session["id"].as_str().unwrap().to_owned();
    let pid = u32::try_from(session["pid"].as_u64().unwrap()).unwrap();
    (id, pid)
}

fn session_document(id: &str, pid: u32) -> Value {
    serde_json::json!({ "id": id, "state": "running", "pid": pid })
}

fn holder_is_live(sandbox: &Sandbox, id: &str) -> bool {
    sandbox
        .holders()
        .iter()
        .any(|holder| holder.session_id.as_str() == id && holder.state == HolderState::Live)
}

#[test]
fn a_session_survives_sigkill_of_the_daemon_and_a_new_daemon_finds_it() {
    let sandbox = Sandbox::new();

    let (id, pid) = new_session(&sandbox);
    assert_eq!(sandbox.sessions(), [session_document(&id, pid)]);

    let first_daemon = sandbox.daemon_pid();
    kill_process(first_daemon);
    wait_for("the daemon is gone", || {
        (!process_runs(first_daemon)).then_some(())
    });
    assert!(holder_is_live(&sandbox, &id));
    assert!(process_runs(pid));

    // `ls` starts a new daemon, which finds the holder.
    assert_eq!(sandbox.sessions(), [session_document(&id, pid)]);
    assert_ne!(sandbox.daemon_pid(), first_daemon);

    let killed = sandbox.nomux_ok(&["kill", &id, "--json"]).json();
    assert_eq!(killed["killed"], id.as_str());
    assert_eq!(sandbox.sessions(), NO_SESSIONS);
    assert!(!process_runs(pid));
    wait_for("the holder removed its directory", || {
        sandbox.holders().is_empty().then_some(())
    });

    let again = sandbox.nomux(&["kill", &id]);
    assert_eq!(again.code, 2, "{again:?}");
    assert_eq!(again.stdout, "");
    assert!(again.stderr.contains(&id), "{again:?}");
}

#[test]
fn a_session_survives_sigkill_of_the_process_group_of_the_daemon() {
    let sandbox = Sandbox::new();
    let (id, pid) = new_session(&sandbox);

    // The daemon leads its own process group. A service manager that stops
    // a daemon without a control group ends this group.
    let daemon = sandbox.daemon_pid();
    kill_process_group(daemon);
    wait_for("the daemon is gone", || {
        (!process_runs(daemon)).then_some(())
    });

    assert!(holder_is_live(&sandbox, &id));
    assert!(process_runs(pid));
    assert_eq!(sandbox.sessions(), [session_document(&id, pid)]);
}

#[test]
fn daemon_stop_keeps_the_sessions_and_is_idempotent() {
    let sandbox = Sandbox::new();

    let stopped = sandbox.nomux(&["daemon", "status", "--json"]);
    assert_eq!(stopped.code, 2, "{stopped:?}");
    assert_eq!(stopped.json()["daemon"]["running"], false);
    assert!(
        !sandbox.runtime_directory().exists(),
        "`daemon status` must not start a daemon"
    );

    let (id, pid) = new_session(&sandbox);
    let status = sandbox.nomux_ok(&["daemon", "status", "--json"]).json();
    assert_eq!(status["daemon"]["running"], true);
    assert_eq!(status["daemon"]["sessions"], 1);

    assert_eq!(
        sandbox.nomux_ok(&["daemon", "stop", "--json"]).json()["stopped"],
        true
    );
    assert_eq!(sandbox.nomux(&["daemon", "status"]).code, 2);
    assert_eq!(
        sandbox.nomux_ok(&["daemon", "stop", "--json"]).json()["stopped"],
        false
    );

    assert_eq!(sandbox.sessions(), [session_document(&id, pid)]);
}

#[test]
fn a_holder_that_does_not_run_leaves_the_list_and_the_runtime_directory() {
    let sandbox = Sandbox::new();
    let (id, _) = new_session(&sandbox);

    // A holder directory of a process that ended before the daemon started.
    assert_eq!(
        sandbox.nomux_ok(&["daemon", "stop"]).stdout,
        "nomuxd stopped\n"
    );
    let stale = sandbox.runtime_directory().join("stale");
    fs::create_dir(&stale).unwrap();
    fs::write(stale.join(PID_FILE), "2147483647 1\n").unwrap();
    assert_eq!(sandbox.sessions().len(), 1);
    assert!(!stale.exists());

    // A holder that ends while the daemon is connected to it.
    kill_process(sandbox.holder_pid(&id));
    wait_for("the session left the list", || {
        sandbox.sessions().is_empty().then_some(())
    });
    wait_for("the daemon removed the holder directory", || {
        sandbox.holders().is_empty().then_some(())
    });
}

#[test]
fn a_session_that_cannot_start_is_an_error_and_leaves_nothing() {
    let sandbox = Sandbox::new();

    let no_program = sandbox.nomux(&["new", "--cmd", "/nonexistent/program"]);
    assert_eq!(no_program.code, 1, "{no_program:?}");
    assert!(
        no_program.stderr.contains("did not start"),
        "{no_program:?}"
    );

    let no_directory = sandbox.nomux(&["new", "--cwd", "/nonexistent/directory"]);
    assert_eq!(no_directory.code, 1, "{no_directory:?}");

    assert_eq!(sandbox.sessions(), NO_SESSIONS);
    assert_eq!(sandbox.holders(), []);
}

#[test]
fn the_command_gets_its_arguments_and_the_working_directory() {
    let sandbox = Sandbox::new();
    let work = tempfile::tempdir().unwrap();
    let work_path = work.path().canonicalize().unwrap();
    let marker = work_path.join("marker");
    let created = sandbox.nomux_ok(&[
        "new",
        "--cwd",
        work_path.to_str().unwrap(),
        "--cmd",
        "sh",
        "-c",
        "pwd > marker; sleep 600",
    ]);
    let id = created.stdout.trim();
    assert_eq!(
        sandbox.nomux_ok(&["ls", "--quiet"]).stdout,
        format!("{id}\n")
    );

    let written = wait_for("the command wrote the marker", || {
        fs::read_to_string(&marker)
            .ok()
            .filter(|text| !text.is_empty())
    });
    assert_eq!(written.trim(), work_path.to_str().unwrap());
}
