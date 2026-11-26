//! The holder pool of the daemon with the real binaries: a new session with
//! the shell takes a pooled holder and works as a session with a new
//! holder, the pool starts only after the first request of a client and
//! empties after its idle time, and no pooled holder stays without its
//! daemon.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_client::{Client, ClientKind, NewSession};
use nomux_end_to_end::{Sandbox, binary, kill_process, process_runs, wait_for};
use nomux_holder_protocol::discovery::{
    HolderState, PID_FILE, ProcessIdentity, pooled_holder_directory, scan_pooled,
};

/// The pool of the default configuration.
const POOL_SIZE: usize = 2;
/// The target of `bench/README.md` for a new session from the pool.
const NEW_SESSION_TARGET: Duration = Duration::from_millis(10);

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime starts")
        .block_on(future)
}

/// The identifiers of the live pooled holders, when the pool has `count`.
fn wait_for_pool(sandbox: &Sandbox, count: usize) -> HashSet<String> {
    wait_for("the pool has its holders", || {
        let live: HashSet<String> = scan_pooled(&sandbox.runtime_directory())
            .expect("the runtime directory can be read")
            .into_iter()
            .filter(|holder| holder.state == HolderState::Live)
            .map(|holder| holder.session_id.to_string())
            .collect();
        (live.len() == count).then_some(live)
    })
}

fn pooled_holder_pid(sandbox: &Sandbox, id: &str) -> u32 {
    let directory = pooled_holder_directory(
        &sandbox.runtime_directory(),
        &id.parse().expect("a session identifier"),
    );
    let identity: ProcessIdentity = fs::read_to_string(directory.join(PID_FILE))
        .expect("the pooled holder has a pid file")
        .parse()
        .expect("the pid file is valid");
    identity.pid
}

/// Writes `line` to the terminal of the session `id`, as typed input.
fn type_line(sandbox: &Sandbox, id: &str, line: &str) {
    block_on(async {
        let mut client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
            .await
            .expect("the daemon answers");
        client
            .send_input(id, format!("{line}\n").as_bytes())
            .await
            .expect("the session takes the input");
    });
}

fn wait_for_capture(sandbox: &Sandbox, id: &str, text: &str) {
    wait_for(&format!("the output of session {id} has {text:?}"), || {
        sandbox
            .nomux_ok(&["capture", id])
            .stdout
            .contains(text)
            .then_some(())
    });
}

fn new_session_in(sandbox: &Sandbox, directory: &Path) -> String {
    let mut command = sandbox.nomux_command(&["new", "--quiet"]);
    let output = command
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .expect("the CLI starts");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("text")
        .trim()
        .to_owned()
}

/// A working directory with a space. The path has no symbolic link: the
/// CLI sends the current directory, and `/tmp` is a link on macOS.
fn project_directory(sandbox: &Sandbox) -> PathBuf {
    let directory = sandbox.home().join("a project");
    fs::create_dir(&directory).expect("the directory can be made");
    fs::canonicalize(directory).expect("the directory exists")
}

#[test]
fn a_session_from_the_pool_works_as_a_session_with_a_new_holder() {
    let sandbox = Sandbox::new();
    let project = project_directory(&sandbox);
    let _ = sandbox.sessions();
    let pooled = wait_for_pool(&sandbox, POOL_SIZE);

    let id = new_session_in(&sandbox, &project);

    assert!(pooled.contains(&id), "session {id} is not from {pooled:?}");
    let listed = sandbox.sessions();
    assert_eq!(listed.len(), 1, "a pooled holder is a session: {listed:?}");
    assert_eq!(listed[0]["state"], "running");
    assert_eq!(listed[0]["rows"], 24);
    assert_eq!(listed[0]["columns"], 80);
    assert_eq!(listed[0]["cwd"], project.to_str().expect("text"));
    assert_eq!(listed[0]["epoch"], 1);

    // The shell is in the directory of the session and has the variables of
    // the session. The token proves the session to the daemon.
    type_line(
        &sandbox,
        &id,
        &format!(
            "echo \"[$PWD|$NOMUX_SESSION_ID|${{#NOMUX_TOKEN}}]\"; {} status set working",
            binary("nomux").display()
        ),
    );
    wait_for_capture(&sandbox, &id, &format!("[{}|{id}|32]", project.display()));
    wait_for("the self-report arrives", || {
        let tree = sandbox.nomux_ok(&["status", "--tree", "--json"]).json();
        tree["status"]["sessions"]
            .as_array()?
            .iter()
            .any(|run| run["id"] == id.as_str() && run["status"]["source"] == "self_report")
            .then_some(())
    });
    // An attach shows the screen: the prompt in the directory of the
    // session, and the output of a key that the terminal types.
    let mut terminal = sandbox.nomux_in_terminal(&["attach", &id]);
    terminal.wait_for_text(&format!("[{}|{id}|32]", project.display()));
    terminal.type_keys(b"echo at''tached\r");
    terminal.wait_for_text("attached");
    drop(terminal);

    // The pool fills again after the take.
    let refilled = wait_for_pool(&sandbox, POOL_SIZE);
    assert!(!refilled.contains(&id));

    // A restart of the host: the session is dormant, and it starts again
    // in its directory with the journal of the first epoch.
    sandbox.kill_each_process();
    let listed = sandbox.sessions();
    assert_eq!(listed[0]["id"], id.as_str());
    assert_eq!(listed[0]["state"], "dormant");
    let _ = sandbox.nomux_ok(&["start", &id]);
    type_line(&sandbox, &id, "echo \"(again in $PWD)\"");
    wait_for_capture(&sandbox, &id, &format!("(again in {})", project.display()));
}

/// The time from the request of a client that is connected to the response:
/// the session then accepts input. The median of 11 sessions, each with a
/// full pool, also in a debug build.
#[test]
fn a_session_from_the_pool_opens_in_less_than_10_ms() {
    let sandbox = Sandbox::new();
    let project = project_directory(&sandbox);
    let _ = sandbox.sessions();
    let mut times = Vec::new();
    for _ in 0..11 {
        let pooled = wait_for_pool(&sandbox, POOL_SIZE);
        let (id, time) = block_on(async {
            let mut client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
                .await
                .expect("the daemon answers");
            let start = Instant::now();
            let session = client
                .create_session(NewSession {
                    command: Vec::new(),
                    working_directory: Some(project.clone()),
                })
                .await
                .expect("the session starts");
            (session.id, start.elapsed())
        });
        assert!(pooled.contains(&id), "session {id} is not from {pooled:?}");
        times.push(time);
    }
    times.sort();
    assert!(times[times.len() / 2] < NEW_SESSION_TARGET, "{times:?}");
}

#[test]
fn no_pooled_holder_runs_before_the_first_request_or_after_the_idle_time() {
    let mut sandbox = Sandbox::new();
    sandbox.write_configuration("[pool]\nidle_time = \"2s\"\n");
    let daemon = Command::new(binary("nomuxd"))
        .arg("--detach")
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.home().join("config"))
        .env("SHELL", "/bin/sh")
        .status()
        .expect("the daemon starts");
    assert!(daemon.success());
    wait_for("the daemon listens", || {
        sandbox.daemon_socket().exists().then_some(())
    });
    thread::sleep(Duration::from_millis(500));
    assert!(
        scan_pooled(&sandbox.runtime_directory())
            .unwrap()
            .is_empty(),
        "a pooled holder started before a request"
    );

    let _ = sandbox.sessions();
    let pooled = wait_for_pool(&sandbox, POOL_SIZE);
    let pids: Vec<u32> = pooled
        .iter()
        .map(|id| pooled_holder_pid(&sandbox, id))
        .collect();

    // No request for the idle time: the pool empties, and its holders and
    // their shells end.
    wait_for("the pool is empty", || {
        scan_pooled(&sandbox.runtime_directory())
            .unwrap()
            .is_empty()
            .then_some(())
    });
    assert!(pids.iter().all(|pid| !process_runs(*pid)));
    assert_eq!(sandbox.sessions().len(), 0, "a pooled holder is a session");
}

#[test]
fn the_pooled_holders_end_with_their_daemon_and_a_new_daemon_has_its_own() {
    let sandbox = Sandbox::new();
    let _ = sandbox.sessions();
    let earlier = wait_for_pool(&sandbox, POOL_SIZE);
    let pids: Vec<u32> = earlier
        .iter()
        .map(|id| pooled_holder_pid(&sandbox, id))
        .collect();

    // `SIGKILL`: the daemon does nothing at its end. The pipes close.
    kill_process(sandbox.daemon_pid());
    wait_for("the pooled holders end", || {
        pids.iter().all(|pid| !process_runs(*pid)).then_some(())
    });

    assert_eq!(sandbox.sessions().len(), 0, "a pooled holder is a session");
    let later = wait_for_pool(&sandbox, POOL_SIZE);
    assert!(earlier.is_disjoint(&later), "{earlier:?} and {later:?}");
}
