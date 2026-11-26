//! The agent adapters with the real binaries: `nomux hook` in a session,
//! the test adapter extension of `testtool`, the adapter source of the
//! status, the events of an extension, and `nomux hooks`.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Child, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nomux_client::{Client, ClientKind};
use nomux_end_to_end::{Sandbox, TEST_ADAPTER, adapter_package, binary, wait_for};
use serde_json::Value;

/// A hook that ends later than this blocks its tool: the budget of the
/// command is 100 ms, and a loaded test computer is slow.
const AT_ONCE: Duration = Duration::from_secs(1);

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime starts")
        .block_on(future)
}

/// A sandbox with the test adapter installed and a session with a shell.
/// Returns the sandbox and the session identifier.
fn sandbox_with_adapter() -> (Sandbox, String) {
    let sandbox = Sandbox::new();
    let package = adapter_package(&sandbox.home().join("packages"));
    let package = package.display().to_string();
    let _ = sandbox.nomux_ok(&["ext", "install", &package, "--grant-all"]);
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh"])
        .stdout
        .trim()
        .to_owned();
    (sandbox, id)
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

/// Types the hook `event` of `testtool` with the JSON `data` into the shell
/// of the session `id`: the hook runs in the session, as a hook of a tool
/// does.
fn hook_in_session(sandbox: &Sandbox, id: &str, event: &str, data: &str) {
    let nomux = binary("nomux");
    let command = format!(
        "printf '%s' '{data}' | {} hook testtool {event}",
        nomux.display()
    );
    type_line(sandbox, id, &command);
}

/// Types a report of the Program Status Protocol into the shell of the
/// session `id`.
fn program_status(sandbox: &Sandbox, id: &str, fields: &str) {
    type_line(
        sandbox,
        id,
        &format!("printf '\\033]7501;{fields}\\033\\\\'"),
    );
}

fn status_tree(sandbox: &Sandbox) -> Value {
    sandbox.nomux_ok(&["status", "--tree", "--json"]).json()["status"].clone()
}

fn wait_for_run(sandbox: &Sandbox, id: &str, what: &str, check: impl Fn(&Value) -> bool) -> Value {
    wait_for(what, || {
        let tree = status_tree(sandbox);
        let run = tree["sessions"]
            .as_array()?
            .iter()
            .find(|run| run["id"] == id)?
            .clone();
        check(&run).then_some(run)
    })
}

/// Runs `nomux hook testtool <event>` outside a session with `variables`
/// and the standard input `data`, and gives its output and its time.
fn hook_outside(
    sandbox: &Sandbox,
    variables: &[(&str, &str)],
    event: &str,
    data: &[u8],
) -> (Output, Duration) {
    let mut command = sandbox.nomux_command(&["hook", "testtool", event]);
    command
        .env_remove("NOMUX_SESSION_ID")
        .env_remove("NOMUX_SOCK")
        .env_remove("NOMUX_TOKEN")
        .envs(variables.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let mut hook = command.spawn().expect("the CLI starts");
    let mut stdin = hook.stdin.take().expect("the input is piped");
    // The hook can end before it reads all: the tool does not care.
    let _ = stdin.write_all(data);
    drop(stdin);
    let output = hook.wait_with_output().expect("the CLI ends");
    (output, started.elapsed())
}

/// The token of the session `id`: its shell writes it to a file.
fn token_of(sandbox: &Sandbox, id: &str) -> String {
    let file = sandbox.home().join("token");
    type_line(
        sandbox,
        id,
        &format!("printf %s \"$NOMUX_TOKEN\" > {}", file.display()),
    );
    wait_for("the shell wrote the token", || {
        fs::read_to_string(&file)
            .ok()
            .filter(|token| token.len() == 32)
    })
}

/// `nomux events --follow --json --filter extension_event` and the events
/// that it prints.
fn follow_extension_events(sandbox: &Sandbox) -> (Child, Receiver<Value>) {
    let mut command = sandbox.nomux_command(&[
        "events",
        "--follow",
        "--json",
        "--filter",
        "extension-event",
    ]);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut events = command.spawn().expect("the CLI starts");
    let stdout = events.stdout.take().expect("the output is piped");
    let (sender, printed) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let document: Value = serde_json::from_str(&line.unwrap()).expect("each line is JSON");
            if sender.send(document["event"].clone()).is_err() {
                return;
            }
        }
    });
    (events, printed)
}

#[test]
fn a_hook_event_of_a_session_becomes_the_adapter_status_by_the_merge_table() {
    let (sandbox, id) = sandbox_with_adapter();

    // A firm report of the adapter decides before the program status.
    program_status(&sandbox, &id, "state=done");
    wait_for_run(&sandbox, &id, "the program reports done", |run| {
        run["status"]["source"] == "program_status"
    });
    hook_in_session(
        &sandbox,
        &id,
        "prompt",
        r#"{"model":"m-1","session":"s-1"}"#,
    );
    let run = wait_for_run(&sandbox, &id, "the adapter reports the prompt", |run| {
        run["status"]["source"] == "adapter"
    });
    assert_eq!(run["status"]["state"], "working");
    assert_eq!(run["details"]["model"], "m-1");
    assert_eq!(run["details"]["resume_command"], "testtool --resume s-1");
    let explained = sandbox
        .nomux_ok(&["agent", "explain", &id, "--json"])
        .json();
    let first = &explained["explain"]["sources"][0];
    assert_eq!(
        first["source"], "adapter",
        "the adapter is the first source"
    );
    assert_eq!(first["state"], "working");

    // A soft working report yields to a state that needs the user, and
    // its tool name still counts.
    program_status(&sandbox, &id, "state=blocked:kind=permission");
    hook_in_session(&sandbox, &id, "tool", r#"{"tool":"edit"}"#);
    let run = wait_for_run(&sandbox, &id, "the tool event yields", |run| {
        run["status"]["tool"]["text"] == "edit"
    });
    assert_eq!(run["status"]["state"], "needs_you");
    assert_eq!(run["status"]["source"], "program_status");
    assert_eq!(run["status"]["tool"]["source"], "adapter");

    // Child work starts and ends.
    hook_in_session(
        &sandbox,
        &id,
        "task",
        r#"{"id":"a1","phase":"start","title":"explore"}"#,
    );
    let run = wait_for_run(&sandbox, &id, "the Child work starts", |run| {
        run["children"][0]["name"] == "a1"
    });
    let child = &run["children"][0];
    assert_eq!(child["status"]["state"], "working");
    assert_eq!(child["status"]["source"], "adapter");
    assert_eq!(child["status"]["title"]["text"], "explore");
    hook_in_session(&sandbox, &id, "task", r#"{"id":"a1","phase":"end"}"#);
    wait_for_run(&sandbox, &id, "the Child work ends", |run| {
        run["children"][0]["status"]["state"] == "done"
    });
    let tree = sandbox.nomux_ok(&["status", "--tree"]).stdout;
    assert!(tree.contains("a1"), "{tree}");
}

#[test]
fn an_ended_session_keeps_its_last_agent_run_with_its_resume_command() {
    // Issue #73: the command of the session is the agent tool, and the
    // session ends with it.
    let (sandbox, id) = sandbox_with_adapter();
    hook_in_session(&sandbox, &id, "prompt", r#"{"session":"s-1"}"#);
    hook_in_session(
        &sandbox,
        &id,
        "task",
        r#"{"id":"a1","phase":"start","title":"explore"}"#,
    );
    wait_for_run(&sandbox, &id, "the Child work runs", |run| {
        run["status"]["state"] == "working" && run["children"][0]["status"]["state"] == "working"
    });

    type_line(&sandbox, &id, "exit");
    let run = wait_for_run(&sandbox, &id, "the run of the ended session", |run| {
        run["status"]["source"] == "session_restore"
    });
    assert_eq!(run["status"]["state"], "done");
    assert_eq!(run["status"]["seen"], false);
    assert_eq!(run["children"][0]["status"]["state"], "done");
    assert_eq!(run["details"]["resume_command"], "testtool --resume s-1");

    let tree = sandbox.nomux_ok(&["status", "--tree"]).stdout;
    assert!(tree.contains(&id) && tree.contains("a1"), "{tree}");

    let explained = sandbox.nomux_ok(&["agent", "explain", &id]).stdout;
    assert!(explained.contains("session restore"), "{explained}");
    assert!(explained.contains("testtool --resume s-1"), "{explained}");
    let run = wait_for_run(&sandbox, &id, "the run after the explanation", |_| true);
    assert_eq!(
        run["status"]["seen"], false,
        "an explanation changes nothing"
    );
}

#[test]
fn a_hook_event_with_a_wrong_or_missing_token_is_refused() {
    let (sandbox, id) = sandbox_with_adapter();
    let token = token_of(&sandbox, &id);
    let socket = sandbox.daemon_socket();
    let socket = socket.to_str().unwrap();

    for wrong in ["0".repeat(32), String::new()] {
        let variables = [
            ("NOMUX_SESSION_ID", id.as_str()),
            ("NOMUX_SOCK", socket),
            ("NOMUX_TOKEN", wrong.as_str()),
        ];
        let (output, _) = hook_outside(&sandbox, &variables, "prompt", b"{}");
        assert!(output.status.success(), "a hook never fails its tool");
        assert_eq!(output.stdout, b"");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("refused"), "{stderr}");
    }
    let explained = sandbox
        .nomux_ok(&["agent", "explain", &id, "--json"])
        .json();
    assert_eq!(explained["explain"]["sources"][0]["reported"], false);

    // The token of the session proves it, also from outside the session.
    let variables = [
        ("NOMUX_SESSION_ID", id.as_str()),
        ("NOMUX_SOCK", socket),
        ("NOMUX_TOKEN", token.as_str()),
    ];
    let (output, _) = hook_outside(&sandbox, &variables, "prompt", b"{}");
    assert_eq!(output.stderr, b"", "{output:?}");
    wait_for_run(&sandbox, &id, "the adapter reports", |run| {
        run["status"]["source"] == "adapter"
    });
}

#[test]
fn a_hook_ends_at_once_without_a_session_without_a_daemon_and_with_a_daemon_that_does_not_answer() {
    let sandbox = Sandbox::new();
    let silent_socket = sandbox.home().join("silent.sock");
    // The listener accepts connections and never answers.
    let _silent = UnixListener::bind(&silent_socket).unwrap();
    let silent_socket = silent_socket.to_str().unwrap();
    let missing_socket = sandbox.home().join("missing.sock");
    let missing_socket = missing_socket.to_str().unwrap();
    let large = vec![b'x'; 2 * 1024 * 1024];

    let session = |socket| vec![("NOMUX_SESSION_ID", "s1"), ("NOMUX_SOCK", socket)];
    let cases = [
        ("outside a session", Vec::new(), &b"{}"[..]),
        ("without a daemon", session(missing_socket), b"{}"),
        (
            "with a daemon that does not answer",
            session(silent_socket),
            b"{}",
        ),
        (
            "with an input that is too large",
            session(silent_socket),
            &large,
        ),
    ];
    for (case, variables, input) in cases {
        let (output, time) = hook_outside(&sandbox, &variables, "prompt", input);
        assert!(output.status.success(), "{case}: {output:?}");
        assert_eq!(output.stdout, b"", "{case}");
        assert_eq!(output.stderr, b"", "{case}: {output:?}");
        assert!(time < AT_ONCE, "{case}: the hook took {time:?}");
    }
    // The hook started no daemon.
    assert!(!sandbox.daemon_socket().exists());
}

#[test]
fn an_event_of_an_extension_reaches_a_client_that_follows_the_events() {
    let (sandbox, id) = sandbox_with_adapter();
    let (mut events, printed) = follow_extension_events(&sandbox);

    // The CLI subscribes after some time: the session sends hooks until
    // an event comes.
    let event = (0..20)
        .find_map(|_| {
            hook_in_session(&sandbox, &id, "tool", r#"{"tool":"read"}"#);
            printed.recv_timeout(Duration::from_secs(1)).ok()
        })
        .expect("`events --follow` prints the event of the adapter");
    events.kill().unwrap();
    let _ = events.wait();

    assert_eq!(event["type"], "extension_event");
    assert_eq!(event["extension"]["id"], TEST_ADAPTER);
    assert_eq!(event["extension"]["name"], "mapped");
    let payload = STANDARD
        .decode(event["extension"]["payload_base64"].as_str().unwrap())
        .unwrap();
    // The payload is the message `Mapped { event: "tool" }`.
    assert_eq!(payload, b"\x0a\x04tool");
}

/// The settings file of `testtool` in the sandbox.
fn settings_file(sandbox: &Sandbox) -> std::path::PathBuf {
    sandbox.home().join(".testtool").join("settings.json")
}

fn hooks_state(sandbox: &Sandbox) -> Value {
    let status = sandbox.nomux_ok(&["hooks", "status", "--json"]).json();
    let tools = status["hooks"].as_array().unwrap().clone();
    let tool = tools
        .iter()
        .find(|tool| tool["tool"] == "testtool")
        .unwrap();
    assert_eq!(tool["adapter"], TEST_ADAPTER);
    tool["state"].clone()
}

#[test]
fn hooks_install_writes_only_with_apply_and_uninstall_restores_each_byte() {
    let (sandbox, _) = sandbox_with_adapter();
    let file = settings_file(&sandbox);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    // Formatting that a write of nomux does not make: the bytes must come
    // back as they are.
    let original =
        b"{ \"theme\":\"dark\",\n\t\"hooks\": {\"prompt\": [ {\"command\": \"other\"} ]} }";
    fs::write(&file, original).unwrap();
    assert_eq!(hooks_state(&sandbox), "missing");

    let dry_run = sandbox.nomux_ok(&["hooks", "install", "testtool", "--dry-run"]);
    assert!(dry_run.stdout.contains("\n+"), "{dry_run:?}");
    assert!(
        dry_run.stdout.contains("nomux hook testtool prompt"),
        "{dry_run:?}"
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        original,
        "a dry run writes nothing"
    );
    let default = sandbox.nomux_ok(&["hooks", "install", "testtool"]);
    assert_eq!(default.stdout, dry_run.stdout, "a dry run is the default");
    assert_eq!(fs::read(&file).unwrap(), original);

    let _ = sandbox.nomux_ok(&["hooks", "install", "testtool", "--apply"]);
    let installed: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(installed["theme"], "dark");
    assert_eq!(installed["hooks"]["prompt"][0]["command"], "other");
    assert_eq!(
        installed["hooks"]["prompt"][1]["command"],
        "nomux hook testtool prompt"
    );
    assert_eq!(hooks_state(&sandbox), "installed");
    let again = sandbox.nomux_ok(&["hooks", "install", "testtool", "--apply"]);
    assert!(again.stdout.contains("nothing to change"), "{again:?}");

    let uninstall = sandbox.nomux_ok(&["hooks", "uninstall", "testtool"]);
    assert!(uninstall.stdout.contains("dry run"), "{uninstall:?}");
    assert_ne!(fs::read(&file).unwrap(), original);
    let _ = sandbox.nomux_ok(&["hooks", "uninstall", "testtool", "--apply"]);
    assert_eq!(fs::read(&file).unwrap(), original);
    assert_eq!(hooks_state(&sandbox), "missing");
}

#[test]
fn an_uninstall_after_a_manual_edit_removes_only_the_entries_of_nomux() {
    let (sandbox, _) = sandbox_with_adapter();
    let file = settings_file(&sandbox);

    // No file before the install: the install makes it.
    let _ = sandbox.nomux_ok(&["hooks", "install", "testtool", "--apply"]);
    let mut edited: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    edited["mine"] = Value::from(true);
    edited["hooks"]["prompt"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"command": "my own hook"}));
    fs::write(&file, serde_json::to_vec(&edited).unwrap()).unwrap();

    let uninstall = sandbox.nomux_ok(&["hooks", "uninstall", "testtool", "--apply"]);
    assert!(
        uninstall.stdout.contains("only the entries of nomux"),
        "{uninstall:?}"
    );
    let after: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(after["mine"], true);
    assert_eq!(
        after["hooks"]["prompt"],
        serde_json::json!([{"command": "my own hook"}])
    );
    assert_eq!(after["hooks"]["tool"], serde_json::json!([]));
}

#[test]
fn an_uninstall_of_an_install_without_a_file_removes_the_file() {
    let (sandbox, _) = sandbox_with_adapter();
    let file = settings_file(&sandbox);

    let _ = sandbox.nomux_ok(&["hooks", "install", "testtool", "--apply"]);
    assert!(file.is_file());
    let _ = sandbox.nomux_ok(&["hooks", "uninstall", "testtool", "--apply"]);

    assert!(!file.exists());
    let missing = sandbox.nomux(&["hooks", "uninstall", "testtool"]);
    assert_eq!(missing.code, 2, "nothing is installed: {missing:?}");
}
