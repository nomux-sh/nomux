//! The status of the sessions with the real binaries: a program reports its
//! status in the terminal stream, a program reports it with
//! `nomux status set`, and the daemon gives it with `nomux status`,
//! `nomux agent explain` and `nomux events --follow`.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nomux_client::{AttachOptions, Client, ClientKind};
use nomux_end_to_end::{Sandbox, binary, kill_process, process_runs, wait_for};
use rustix::process::{Pid, Signal, kill_process as send_signal};
use serde_json::Value;

/// The exit code of a request that the daemon refused: the token is wrong.
const NO_PERMISSION: i32 = 4;

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime starts")
        .block_on(future)
}

/// Starts a session whose command is the status program, and waits until
/// the terminal answered its query.
fn new_status_program(sandbox: &Sandbox) -> String {
    let program = binary("nomux-test-status-program");
    let program = program.to_str().expect("the path is text");
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", program])
        .stdout
        .trim()
        .to_owned();
    wait_for("the status program got the answer to its query", || {
        let captured = sandbox.nomux_ok(&["capture", &id, "--json"]).json();
        captured["capture"]["output"]
            .as_str()?
            .contains("ready")
            .then_some(())
    });
    id
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

/// A report of the status program with a message.
fn report(fields: &str, message: &str) -> String {
    format!("{fields}:msg={}", STANDARD.encode(message))
}

fn status_tree(sandbox: &Sandbox) -> Value {
    sandbox.nomux_ok(&["status", "--tree", "--json"]).json()["status"].clone()
}

fn run_of<'tree>(tree: &'tree Value, id: &str) -> &'tree Value {
    tree["sessions"]
        .as_array()
        .expect("the sessions are a list")
        .iter()
        .find(|run| run["id"] == id)
        .unwrap_or_else(|| panic!("session {id} is in {tree}"))
}

/// Waits until the Agent run of the session `id` passes `check`, and
/// returns it.
fn wait_for_run(sandbox: &Sandbox, id: &str, what: &str, check: impl Fn(&Value) -> bool) -> Value {
    wait_for(what, || {
        let tree = status_tree(sandbox);
        let run = run_of(&tree, id).clone();
        check(&run).then_some(run)
    })
}

/// `nomux events --follow --json` in the sandbox, and the events that it
/// prints, one JSON document for each line.
fn follow_events(sandbox: &Sandbox) -> (Child, Receiver<Value>) {
    let mut command = sandbox.nomux_command(&["events", "--follow", "--json"]);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut events = command.spawn().expect("the CLI starts");
    let stdout = events.stdout.take().expect("the output is piped");
    let (sender, printed) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let document: Value = serde_json::from_str(&line.unwrap()).expect("each line is JSON");
            assert_eq!(document["schema_version"], 1);
            if sender.send(document["event"].clone()).is_err() {
                return;
            }
        }
    });
    (events, printed)
}

/// Stops `events --follow` with `SIGINT`, as a person with Ctrl-C does, and
/// returns its exit code.
fn stop_following(mut events: Child) -> i32 {
    let pid = Pid::from_raw(i32::try_from(events.id()).unwrap()).unwrap();
    send_signal(pid, Signal::INT).expect("the CLI gets the signal");
    events
        .wait()
        .unwrap()
        .code()
        .expect("the CLI ended with a code")
}

/// The CLI starts its subscription after some time. The session `id`
/// changes its state until the CLI prints a change: then the subscription
/// exists.
fn wait_until_subscribed(sandbox: &Sandbox, id: &str, printed: &Receiver<Value>) {
    let subscribed = (0..20).any(|attempt| {
        let state = if attempt % 2 == 0 { "done" } else { "idle" };
        type_line(sandbox, id, &format!("state={state}"));
        printed.recv_timeout(Duration::from_secs(1)).is_ok()
    });
    assert!(subscribed, "`events --follow` prints the changes");
}

/// Each source of `agent explain` and if it reports a status.
fn explained_sources(sandbox: &Sandbox, id: &str) -> Vec<(String, bool)> {
    let explained = sandbox.nomux_ok(&["agent", "explain", id, "--json"]).json();
    explained["explain"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| {
            (
                source["source"].as_str().unwrap().to_owned(),
                source["reported"].as_bool().unwrap(),
            )
        })
        .collect()
}

/// The states of the Agent run of the session `id` in the events, without
/// a repeat. The events must be in the sequence of their `seq`.
fn changed_states(printed: &[Value], id: &str) -> Vec<String> {
    let seqs: Vec<u64> = printed
        .iter()
        .map(|event| event["seq"].as_u64().unwrap())
        .collect();
    assert!(seqs.is_sorted(), "{seqs:?}");
    let mut states: Vec<String> = printed
        .iter()
        .filter(|event| event["type"] == "agent_run_changed" && event["session"] == id)
        .map(|event| event["run"]["status"]["state"].as_str().unwrap().to_owned())
        .collect();
    states.dedup();
    states
}

/// Attaches to the session `id` and detaches: the user looked at it.
fn attach_and_leave(sandbox: &Sandbox, id: &str) {
    block_on(async {
        let client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
            .await
            .unwrap();
        let (_output, _input) = client.attach(id, AttachOptions::default()).await.unwrap();
    });
}

#[test]
fn each_step_of_a_program_report_has_its_source_and_its_order() {
    let sandbox = Sandbox::new();
    let agent = new_status_program(&sandbox);
    let other = new_status_program(&sandbox);
    let (events, printed) = follow_events(&sandbox);
    wait_until_subscribed(&sandbox, &other, &printed);

    type_line(&sandbox, &agent, &report("state=working", "reading files"));
    let run = wait_for_run(&sandbox, &agent, "the agent works", |run| {
        run["status"]["state"] == "working"
    });
    assert_eq!(run["status"]["source"], "program_status");
    assert_eq!(run["status"]["message"]["text"], "reading files");
    assert_eq!(run["status"]["guess"], false);

    type_line(&sandbox, &agent, "id=build/test:state=working");
    let run = wait_for_run(&sandbox, &agent, "the agent has child work", |run| {
        run["children"][0]["children"][0]["status"]["state"] == "working"
    });
    let build = &run["children"][0];
    assert_eq!(build["name"], "build");
    assert_eq!(build["status"], Value::Null, "only its child reports");
    assert_eq!(build["children"][0]["name"], "test");
    assert_eq!(build["children"][0]["status"]["source"], "program_status");

    // A session that needs the user decides the aggregate of the host.
    type_line(
        &sandbox,
        &other,
        &report("state=blocked:kind=permission", "run tests?"),
    );
    wait_for_run(
        &sandbox,
        &other,
        "the other session needs the user",
        |run| run["status"]["state"] == "needs_you",
    );
    type_line(
        &sandbox,
        &agent,
        &report("state=blocked:kind=permission", "write files?"),
    );
    let run = wait_for_run(&sandbox, &agent, "the agent needs the user", |run| {
        run["status"]["state"] == "needs_you"
    });
    assert_eq!(run["status"]["kind"], "permission");
    assert_eq!(run["status"]["source"], "program_status");
    // The two need the user: the one that waits longer comes first.
    let tree = status_tree(&sandbox);
    assert_eq!(tree["attention"]["session"], other.as_str());
    assert_eq!(tree["attention"]["status"]["message"]["text"], "run tests?");

    assert_eq!(
        explained_sources(&sandbox, &agent),
        [
            ("adapter", false),
            ("program_status", true),
            ("self_report", false),
            ("session_restore", false),
            ("prompt_marks", false),
            ("foreground_program", false),
        ]
        .map(|(source, reported)| (source.to_owned(), reported))
    );

    type_line(&sandbox, &agent, "state=done");
    type_line(&sandbox, &agent, "id=build/test:state=done");
    let run = wait_for_run(&sandbox, &agent, "the agent is done", |run| {
        run["aggregate"]["state"] == "done"
    });
    assert_eq!(run["status"]["seen"], false);

    assert_eq!(
        stop_following(events),
        0,
        "a signal ends `events --follow` cleanly"
    );
    let printed: Vec<Value> = printed.iter().collect();
    assert_eq!(
        changed_states(&printed, &agent),
        ["working", "needs_you", "done"],
        "{printed:?}"
    );

    attach_and_leave(&sandbox, &agent);
    let run = wait_for_run(&sandbox, &agent, "the result is seen", |run| {
        run["status"]["seen"] == true
    });
    assert_eq!(run["status"]["state"], "done");

    // A new daemon reads the reports from the holder again.
    let daemon = sandbox.daemon_pid();
    kill_process(daemon);
    wait_for("the daemon is gone", || {
        (!process_runs(daemon)).then_some(())
    });
    let run = wait_for_run(&sandbox, &agent, "the new daemon has the report", |run| {
        run["status"]["state"] == "done"
    });
    assert_eq!(run["status"]["source"], "program_status");
    let run = run_of(&status_tree(&sandbox), &other).clone();
    assert_eq!(run["status"]["state"], "needs_you");
}

#[test]
fn a_session_that_lost_its_process_shows_its_work_as_done_until_a_restore() {
    let sandbox = Sandbox::new();
    let id = new_status_program(&sandbox);
    type_line(
        &sandbox,
        &id,
        &report("state=blocked:kind=question", "which file?"),
    );
    wait_for_run(&sandbox, &id, "the program waits for the user", |run| {
        run["status"]["state"] == "needs_you"
    });

    // The holder ends as at a restart of the host, and the daemon runs on.
    let holder = sandbox.holder_pid(&id);
    kill_process(holder);
    let run = wait_for_run(&sandbox, &id, "the session lost its process", |run| {
        run["status"]["source"] == "session_restore"
    });
    assert_eq!(run["status"]["state"], "done", "no program waits any more");
    assert_eq!(run["status"]["message"], Value::Null);
    assert_eq!(sandbox.sessions()[0]["state"], "dormant");

    // After a restore, the status comes from the new process.
    let _ = sandbox.nomux_ok(&["start", &id]);
    let run = wait_for_run(&sandbox, &id, "the new process has no report", |run| {
        run["status"]["source"] == "none"
    });
    assert_eq!(run["status"]["state"], "idle");
}

#[test]
fn only_a_process_of_the_session_can_report_its_status() {
    let sandbox = Sandbox::new();
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh"])
        .stdout
        .trim()
        .to_owned();
    let nomux = binary("nomux");
    let token_file = sandbox.home().join("token");
    type_line(
        &sandbox,
        &id,
        &format!(
            "printf %s \"$NOMUX_TOKEN\" > {}; {} status set working 'tests run'",
            token_file.display(),
            nomux.display()
        ),
    );
    // While `nomux status set` runs, the foreground program is a guess of
    // the same state. The self-report stays after it.
    let run = wait_for_run(&sandbox, &id, "the self-report arrives", |run| {
        run["status"]["source"] == "self_report"
    });
    assert_eq!(run["status"]["state"], "working");
    assert_eq!(run["status"]["message"]["text"], "tests run");

    let token = fs::read_to_string(&token_file).unwrap();
    assert_eq!(token.len(), 32);
    let socket = sandbox.daemon_socket();
    let outside = |token: &str| {
        let mut command = sandbox.nomux_command(&["status", "set", "done"]);
        command
            .env("NOMUX_SESSION_ID", &id)
            .env("NOMUX_SOCK", &socket)
            .env("NOMUX_TOKEN", token)
            .stdin(Stdio::null());
        command.output().unwrap().status.code().unwrap()
    };
    assert_eq!(outside(&"0".repeat(32)), NO_PERMISSION);
    assert_eq!(outside(""), NO_PERMISSION);
    assert_eq!(outside(&token), 0, "the token of the session proves it");
    let run = run_of(&status_tree(&sandbox), &id).clone();
    assert_eq!(run["status"]["state"], "done");

    // The token is in no file of the daemon or of the session, and no
    // command shows it.
    fs::remove_file(&token_file).unwrap();
    assert_no_file_has(sandbox.home(), &token);
    for arguments in [
        &["ls", "--json"][..],
        &["status", "--tree", "--json"],
        &["agent", "explain", &id, "--json"],
    ] {
        assert!(!sandbox.nomux_ok(arguments).stdout.contains(&token));
    }
}

fn assert_no_file_has(directory: &Path, text: &str) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            assert_no_file_has(&path, text);
        } else if let Ok(bytes) = fs::read(&path) {
            let found = bytes
                .windows(text.len())
                .any(|window| window == text.as_bytes());
            assert!(!found, "{} has the token", path.display());
        }
    }
}
