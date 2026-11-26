//! Sessions without a process, with the real binaries: after each holder
//! and the daemon got `SIGKILL`, as a restart of the host does, a new
//! daemon has the sessions as dormant sessions and starts them on demand.
//! An ended session leaves after its retention time, and a damaged state
//! directory does not stop the daemon.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use nomux_end_to_end::{Sandbox, process_runs, wait_for};
use nomux_holder_protocol::discovery::HolderState;
use nomux_holder_protocol::journal::{JournalReader, segment_files};
use nomux_holder_protocol::session_state::{JOURNAL_DIRECTORY, METADATA_FILE};
use serde_json::Value;

const RESTORED_MARK: &str = "nomux: the session started again";

/// Starts a session with a shell in `directory` and returns its identifier
/// and the process ID of the shell.
fn new_shell(sandbox: &Sandbox, directory: &Path) -> (String, u32) {
    let directory = directory.to_str().unwrap();
    let created = sandbox
        .nomux_ok(&["new", "--json", "--cwd", directory, "--cmd", "sh"])
        .json();
    let session = &created["session"];
    let id = session["id"].as_str().unwrap().to_owned();
    let pid = u32::try_from(session["pid"].as_u64().unwrap()).unwrap();
    (id, pid)
}

/// The session `id` of `nomux ls --all --json`.
fn listed(sandbox: &Sandbox, id: &str) -> Option<Value> {
    let list = sandbox.nomux_ok(&["ls", "--all", "--json"]).json();
    list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["id"] == id)
        .cloned()
}

fn state_of(sandbox: &Sandbox, id: &str) -> String {
    let session = listed(sandbox, id).unwrap_or_else(|| panic!("session {id} is in the list"));
    session["state"].as_str().unwrap().to_owned()
}

fn has_live_holder(sandbox: &Sandbox, id: &str) -> bool {
    sandbox
        .holders()
        .iter()
        .any(|holder| holder.session_id.as_str() == id && holder.state == HolderState::Live)
}

#[test]
fn after_a_restart_of_the_host_the_sessions_are_dormant_and_start_on_demand() {
    let sandbox = Sandbox::new();
    let project = sandbox.home().join("project");
    fs::create_dir(&project).unwrap();
    let (on_focus, on_focus_pid) = new_shell(&sandbox, &project);
    let (auto, auto_pid) = new_shell(&sandbox, sandbox.home());
    let (manual, manual_pid) = new_shell(&sandbox, sandbox.home());
    let _ = sandbox.nomux_ok(&["restore-policy", &auto, "auto"]);
    let _ = sandbox.nomux_ok(&["restore-policy", &manual, "manual"]);

    // The first session gets output and a window size before the restart.
    let mut terminal = sandbox.nomux_in_window(&["attach", &on_focus], 30, 100);
    terminal.wait_for_text("$");
    terminal.type_keys(b"echo before-$((6 * 7))\r");
    terminal.wait_for_text("before-42");
    terminal.type_keys(b"\x1cd");
    assert_eq!(terminal.wait_for_exit(), 0);
    let before = listed(&sandbox, &on_focus).unwrap();
    // A holder writes output to its journal one second after it read it.
    // A restart of the host before that loses this output.
    let journal = sandbox
        .session_state_directory(&on_focus)
        .join(JOURNAL_DIRECTORY);
    wait_for("the output is in the files of the journal", || {
        let on_disk = JournalReader::open(&journal).unwrap().read(1, 0, 65536);
        on_disk
            .unwrap()
            .filter(|range| range.bytes.windows(9).any(|bytes| bytes == b"before-42"))
    });

    sandbox.kill_each_process();
    for pid in [on_focus_pid, auto_pid, manual_pid] {
        wait_for("the command of the session is gone", || {
            (!process_runs(pid)).then_some(())
        });
    }

    // `ls` starts a new daemon. It lists the sessions with what the earlier
    // holders wrote about them, and it starts no process for them.
    let dormant = listed(&sandbox, &on_focus).unwrap();
    assert_eq!(dormant["state"], "dormant");
    assert_eq!(dormant["pid"], Value::Null);
    assert_eq!(dormant["command"], serde_json::json!(["sh"]));
    assert_eq!(dormant["cwd"], project.to_str().unwrap());
    assert_eq!(dormant["rows"], 30);
    assert_eq!(dormant["columns"], 100);
    assert_eq!(dormant["epoch"], 1);
    assert_eq!(dormant["restore_policy"], "on-focus");
    assert_eq!(dormant["started_at"], before["started_at"]);
    assert!(dormant["started_at"].as_u64().unwrap() > 0);
    assert_eq!(state_of(&sandbox, &manual), "dormant");
    for id in [&on_focus, &manual] {
        assert!(!has_live_holder(&sandbox, id));
    }
    // The old output can be read without a start of the session.
    let old_output = sandbox.nomux_ok(&["capture", &on_focus]).stdout;
    assert!(old_output.contains("before-42"), "{old_output}");
    assert!(!has_live_holder(&sandbox, &on_focus));

    // The policy `auto`: the daemon started the session at its start.
    let restarted = wait_for("the session with the policy auto runs", || {
        listed(&sandbox, &auto).filter(|session| session["state"] == "running")
    });
    assert_ne!(restarted["pid"], auto_pid);
    assert_eq!(restarted["epoch"], 2);

    // The policy `manual`: an attach does not start the session.
    let refused = sandbox
        .nomux_in_terminal(&["attach", &manual])
        .wait_for_exit();
    assert_eq!(refused, 1);
    assert_eq!(state_of(&sandbox, &manual), "dormant");
    let started = sandbox.nomux_ok(&["start", &manual, "--json"]).json();
    assert_eq!(started["session"]["state"], "running");
    assert_eq!(started["session"]["epoch"], 2);
    // A second start changes nothing.
    let again = sandbox.nomux_ok(&["start", &manual, "--json"]).json();
    assert_eq!(again["session"]["pid"], started["session"]["pid"]);

    // The policy `on-focus`: the attach starts the session. The terminal
    // shows the old output, then the mark, then the new shell.
    let mut terminal = sandbox.nomux_in_window(&["attach", &on_focus], 30, 100);
    // The mark moves into the scrollback of the terminal with the old
    // output, thus the test reads all the output and not only the screen.
    while !terminal.text_output().contains(RESTORED_MARK) {
        terminal.wait_for_output();
    }
    let rows = terminal.wait_for_rows("the prompt of the new shell", |rows| {
        rows.iter().any(|row| row.contains('$'))
    });
    assert!(
        rows.iter().all(|row| !row.contains("before-42")),
        "the screen of the new shell has old output: {rows:?}"
    );
    terminal.type_keys(b"pwd; echo after-$((7 * 7))\r");
    terminal.wait_for_text("after-49");
    let output = terminal.text_output();
    let old = output.find("before-42").expect("the old output");
    let mark = output.find(RESTORED_MARK).expect("the mark");
    let new = output.find("after-49").expect("the new output");
    assert!(old < mark && mark < new, "{output}");
    // The new shell is in the directory of the session.
    let directory = output[mark..].find(project.to_str().unwrap());
    assert!(directory.is_some(), "{output}");
    terminal.type_keys(b"\x1cd");
    assert_eq!(terminal.wait_for_exit(), 0);

    let restored = listed(&sandbox, &on_focus).unwrap();
    assert_eq!(restored["state"], "running");
    assert_eq!(restored["epoch"], 2);
    assert_ne!(restored["pid"], on_focus_pid);
    // The scrollback has the output of the two epochs.
    let scrollback = sandbox
        .nomux_ok(&["capture", &on_focus, "--lines", "1000"])
        .stdout;
    let old = scrollback.find("before-42").expect("the old output");
    let new = scrollback.find("after-49").expect("the new output");
    assert!(old < new, "{scrollback}");
}

#[test]
fn an_ended_session_stays_for_the_retention_time_and_then_leaves_with_its_state() {
    let mut sandbox = Sandbox::new();
    sandbox.write_configuration("[sessions]\nended_retention = \"6s\"\n");
    let id = sandbox
        .nomux_ok(&[
            "new",
            "--quiet",
            "--",
            "sh",
            "-c",
            "stty -opost; echo last-words; exit 3",
        ])
        .stdout
        .trim()
        .to_owned();
    let state_directory = sandbox.session_state_directory(&id);

    let ended = wait_for("the session ended", || {
        listed(&sandbox, &id).filter(|session| session["state"] == "ended")
    });
    assert_eq!(ended["exit"]["exit_code"], 3);
    let ended_at = ended["exit"]["ended_at"].as_u64().unwrap();
    assert!(ended_at > 0);
    // The output and the state stay.
    assert!(state_directory.is_dir());
    // Without `-opost`, the terminal of macOS sometimes writes two carriage
    // returns before a line feed.
    assert_eq!(sandbox.nomux_ok(&["capture", &id]).stdout, "last-words\n");

    // A new daemon reads the end from the state directory. It keeps the
    // time of the end, thus the retention time does not start again: the
    // two daemons read the clock at the same event, one second apart at
    // most.
    thread::sleep(Duration::from_secs(2));
    let _ = sandbox.nomux_ok(&["daemon", "stop"]);
    let after_restart = listed(&sandbox, &id).expect("the ended session is in the list");
    assert_eq!(after_restart["state"], "ended");
    let read_from_disk = after_restart["exit"]["ended_at"].as_u64().unwrap();
    assert!(read_from_disk.abs_diff(ended_at) <= 1, "{after_restart}");

    // Without the option, the list has only the sessions that can run, and
    // an ended session cannot be used.
    assert_eq!(sandbox.sessions(), [] as [Value; 0]);
    assert_eq!(
        sandbox.nomux_ok(&["daemon", "status", "--json"]).json()["daemon"]["sessions"],
        0
    );
    for command in ["kill", "start", "detach"] {
        assert_eq!(sandbox.nomux(&[command, &id]).code, 2, "{command}");
    }

    wait_for("the retention time is over", || {
        listed(&sandbox, &id).is_none().then_some(())
    });
    assert!(!state_directory.exists());
    assert_eq!(sandbox.nomux(&["capture", &id]).code, 2);
}

#[test]
fn a_damaged_state_directory_gives_a_damaged_session_and_a_daemon_that_works() {
    let sandbox = Sandbox::new();
    let (bad_metadata, _) = new_shell(&sandbox, sandbox.home());
    let (bad_journal, _) = new_shell(&sandbox, sandbox.home());
    let (good, _) = new_shell(&sandbox, sandbox.home());
    sandbox.kill_each_process();

    fs::write(
        sandbox
            .session_state_directory(&bad_metadata)
            .join(METADATA_FILE),
        b"nomux-session 1\nepoch one\n\xff\xfe",
    )
    .unwrap();
    let journal = sandbox
        .session_state_directory(&bad_journal)
        .join(JOURNAL_DIRECTORY);
    let segment = segment_files(&journal).unwrap().remove(0);
    let mut bytes = fs::read(&segment.path).unwrap();
    bytes[..16].fill(0);
    fs::write(&segment.path, bytes).unwrap();
    // A state directory that no holder wrote: its metadata is a FIFO, and
    // a read of a FIFO without a writer does not return.
    let hostile = sandbox.session_state_directory("hostile");
    fs::create_dir_all(&hostile).unwrap();
    let made = Command::new("mkfifo")
        .arg(hostile.join(METADATA_FILE))
        .status()
        .unwrap();
    assert!(made.success());
    // An entry that is not a session.
    fs::write(hostile.parent().unwrap().join("not a session"), b"x").unwrap();

    // `ls` starts a new daemon. It lists each session and says what is
    // damaged.
    let sessions = sandbox.sessions();
    assert_eq!(sessions.len(), 4, "{sessions:?}");
    let damaged = listed(&sandbox, &bad_metadata).unwrap();
    assert_eq!(damaged["state"], "damaged");
    assert!(
        damaged["damage"].as_str().unwrap().contains("metadata"),
        "{damaged}"
    );
    let fifo = listed(&sandbox, "hostile").unwrap();
    assert_eq!(fifo["state"], "damaged");
    assert!(
        fifo["damage"]
            .as_str()
            .unwrap()
            .contains("not a regular file"),
        "{fifo}"
    );
    let partly = listed(&sandbox, &bad_journal).unwrap();
    assert_eq!(partly["state"], "dormant");
    let damage = partly["damage"].as_str().unwrap();
    assert!(damage.contains(&segment.name.file_name()), "{damage}");
    assert_eq!(listed(&sandbox, &good).unwrap()["damage"], Value::Null);
    let table = sandbox.nomux_ok(&["ls"]).stdout;
    assert!(table.contains("damaged"), "{table}");
    assert!(table.contains("NOTE"), "{table}");

    // A session without metadata cannot start. The other sessions work.
    assert_eq!(sandbox.nomux(&["start", &bad_metadata]).code, 1);
    for id in [&bad_journal, &good] {
        let started = sandbox.nomux_ok(&["start", id, "--json"]).json();
        assert_eq!(started["session"]["state"], "running");
    }
    let (new, _) = new_shell(&sandbox, sandbox.home());
    assert_eq!(state_of(&sandbox, &new), "running");

    // `kill` removes a damaged session with its state.
    for id in [bad_metadata.as_str(), "hostile"] {
        let _ = sandbox.nomux_ok(&["kill", id]);
        assert!(listed(&sandbox, id).is_none());
        assert!(!sandbox.session_state_directory(id).exists());
    }
}
