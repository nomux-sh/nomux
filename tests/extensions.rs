//! The extensions with the real binaries: the `nomux ext` commands, the
//! daemon with its extension host, and the test extension of
//! `nomux-ext-host`, which uses the extension SDK.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nomux_end_to_end::{CliRun, Sandbox, extension_package, process_runs, wait_for};
use prost::Message;
use serde_json::{Value, json};

const NO_PERMISSION: i32 = 4;
const NOT_FOUND: i32 = 2;
const ERROR: i32 = 1;

/// The permissions that the manifest of a test package asks for.
const REQUESTED: &str = "process = [\"git\"]\nscopes = [\"sessions:read\"]";

/// The argument and the result of most methods of the test extension.
#[derive(Clone, PartialEq, prost::Message)]
struct Text {
    #[prost(string, tag = "1")]
    text: String,
}

/// The result of the `process` method of the test extension.
#[derive(Clone, PartialEq, prost::Message)]
struct ProcessInfo {
    #[prost(uint32, tag = "1")]
    pid: u32,
}

fn package(sandbox: &Sandbox, id: &str) -> String {
    let directory: PathBuf = extension_package(&sandbox.home().join("packages"), id, REQUESTED);
    directory.display().to_string()
}

/// Installs the test extension `id` with each permission that it asks for.
fn install(sandbox: &Sandbox, id: &str) {
    let _ = sandbox.nomux_ok(&["ext", "install", &package(sandbox, id), "--grant-all"]);
}

fn extensions(sandbox: &Sandbox) -> Vec<Value> {
    let listed = sandbox.nomux_ok(&["ext", "ls", "--json"]).json();
    listed["extensions"].as_array().unwrap().clone()
}

fn state(sandbox: &Sandbox, id: &str) -> String {
    let extensions = extensions(sandbox);
    let extension = extensions.iter().find(|extension| extension["id"] == id);
    extension.unwrap()["state"].as_str().unwrap().to_owned()
}

/// Calls a method of the test extension with a text and gives the run of
/// the CLI. The payload goes through a file, as the bytes of a message.
fn call(sandbox: &Sandbox, id: &str, method: &str, text: &str) -> CliRun {
    let payload = sandbox.home().join(format!("payload-{id}-{method}"));
    let message = Text {
        text: text.to_owned(),
    };
    fs::write(&payload, message.encode_to_vec()).unwrap();
    let payload = format!("@{}", payload.display());
    sandbox.nomux(&["ext", "call", id, method, "--json", "--payload", &payload])
}

/// The payload of the result of a call that succeeded.
fn result_payload(run: &CliRun) -> Vec<u8> {
    assert_eq!(run.code, 0, "{run:?}");
    let encoded = run.json()["call"]["payload_base64"].clone();
    STANDARD.decode(encoded.as_str().unwrap()).unwrap()
}

fn text_result(run: &CliRun) -> String {
    Text::decode(result_payload(run).as_slice()).unwrap().text
}

/// The process ID of the process of the extension `id`. The call starts the
/// process if it does not run.
fn extension_pid(sandbox: &Sandbox, id: &str) -> u32 {
    let run = call(sandbox, id, "process", "");
    ProcessInfo::decode(result_payload(&run).as_slice())
        .unwrap()
        .pid
}

fn new_session(sandbox: &Sandbox, command: &[&str]) -> String {
    let arguments = [&["new", "--quiet", "--"], command].concat();
    sandbox.nomux_ok(&arguments).stdout.trim().to_owned()
}

#[test]
fn an_install_needs_an_approval_and_a_removal_leaves_nothing() {
    let sandbox = Sandbox::new();
    let package = package(&sandbox, "test.git");

    let refused = sandbox.nomux(&["ext", "install", &package]);

    assert_eq!(refused.code, NO_PERMISSION, "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
    for shown in ["process=git", "scope=sessions:read", "--grant-all"] {
        assert!(refused.stderr.contains(shown), "{shown}: {refused:?}");
    }
    assert_eq!(extensions(&sandbox), Vec::<Value>::new());

    // The user approves a part of what the extension asks for.
    let installed = sandbox.nomux_ok(&[
        "ext",
        "install",
        &package,
        "--grant",
        "scope=sessions:read",
        "--json",
    ]);

    let extension = json!({
        "id": "test.git",
        "name": "Test",
        "version": "1.0.0",
        "state": "stopped",
        "reason": null,
        "permissions": {"fs": [], "process": [], "network": false, "scopes": ["sessions:read"]},
    });
    assert_eq!(installed.json()["extension"], extension);
    assert_eq!(extensions(&sandbox), [extension]);

    let removed = sandbox.nomux_ok(&["ext", "rm", "test.git", "--json"]);

    assert_eq!(removed.json()["removed_extension"], "test.git");
    assert_eq!(extensions(&sandbox), Vec::<Value>::new());
    let again = sandbox.nomux(&["ext", "rm", "test.git"]);
    assert_eq!(again.code, NOT_FOUND, "{again:?}");
    let unknown = call(&sandbox, "test.git", "echo", "no");
    assert_eq!(unknown.code, NOT_FOUND, "{unknown:?}");
}

#[test]
fn a_permission_that_the_manifest_does_not_ask_for_is_refused() {
    let sandbox = Sandbox::new();
    let package = package(&sandbox, "test.git");

    let refused = sandbox.nomux(&["ext", "install", &package, "--grant", "network"]);

    assert_eq!(refused.code, ERROR, "{refused:?}");
    assert!(refused.stderr.contains("network"), "{refused:?}");
    assert_eq!(extensions(&sandbox), Vec::<Value>::new());
}

#[test]
fn the_first_call_starts_the_process_and_an_idle_process_stops() {
    let mut sandbox = Sandbox::new();
    sandbox.set_variable("NOMUX_EXTENSION_IDLE_SECONDS", "1");
    install(&sandbox, "test.echo");
    assert_eq!(state(&sandbox, "test.echo"), "stopped");

    let echoed = call(&sandbox, "test.echo", "echo", "from the extension");

    assert_eq!(text_result(&echoed), "from the extension");
    let pid = extension_pid(&sandbox, "test.echo");
    assert!(process_runs(pid));
    wait_for("the idle process stops", || {
        (!process_runs(pid) && state(&sandbox, "test.echo") == "stopped").then_some(())
    });

    // The next call starts a new process.
    assert_ne!(extension_pid(&sandbox, "test.echo"), pid);
}

#[test]
fn the_sessions_service_needs_the_scope_that_the_user_approved() {
    let sandbox = Sandbox::new();
    let session = new_session(&sandbox, &["sleep", "300"]);
    install(&sandbox, "test.with-scope");
    let without_scope = package(&sandbox, "test.without-scope");
    let _ = sandbox.nomux_ok(&["ext", "install", &without_scope, "--grant", "process=git"]);

    let listed = call(&sandbox, "test.with-scope", "session_ids", "");
    let refused = call(&sandbox, "test.without-scope", "session_ids", "");

    assert_eq!(text_result(&listed), session);
    assert_eq!(refused.code, NO_PERMISSION, "{refused:?}");
    assert!(refused.stdout.is_empty(), "{refused:?}");
}

#[test]
fn a_crash_fails_the_call_and_the_next_call_starts_the_extension_again() {
    let sandbox = Sandbox::new();
    let session = new_session(&sandbox, &["sleep", "300"]);
    let daemon = sandbox.daemon_pid();
    install(&sandbox, "test.crash");
    let first_process = extension_pid(&sandbox, "test.crash");

    let crashed = call(&sandbox, "test.crash", "crash", "");

    assert_eq!(crashed.code, ERROR, "{crashed:?}");
    assert!(crashed.stderr.contains("the process ended"), "{crashed:?}");
    assert_eq!(sandbox.daemon_pid(), daemon);
    let sessions = sandbox.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], session.as_str());
    assert_eq!(sessions[0]["state"], "running");
    assert_eq!(state(&sandbox, "test.crash"), "failed");

    // A call during the pause after the failure starts nothing.
    let answer = wait_for("the extension starts again", || {
        let run = call(&sandbox, "test.crash", "echo", "again");
        (run.code == 0).then(|| text_result(&run))
    });
    assert_eq!(answer, "again");
    assert_ne!(extension_pid(&sandbox, "test.crash"), first_process);
}

#[test]
fn after_repeated_failures_the_extension_is_disabled_and_the_list_shows_why() {
    let sandbox = Sandbox::new();
    // The program of this identifier ends at each start.
    install(&sandbox, "test.crash-at-start");

    // The pauses between the five starts are 1, 2, 4 and 8 seconds.
    let deadline = Instant::now() + Duration::from_secs(60);
    while state(&sandbox, "test.crash-at-start") != "disabled" {
        assert!(Instant::now() < deadline, "the extension is not disabled");
        let failed = call(&sandbox, "test.crash-at-start", "echo", "");
        assert_eq!(failed.code, ERROR, "{failed:?}");
        thread::sleep(Duration::from_millis(200));
    }

    let extension = &extensions(&sandbox)[0];
    let reason = extension["reason"].as_str().unwrap();
    assert!(
        reason.contains("the extension failed: the process ended"),
        "{reason}"
    );
    let table = sandbox.nomux_ok(&["ext", "ls"]).stdout;
    assert!(
        table.contains("disabled") && table.contains(reason),
        "{table}"
    );
    let refused = call(&sandbox, "test.crash-at-start", "echo", "");
    assert_eq!(refused.code, ERROR, "{refused:?}");
    assert!(refused.stderr.contains("is disabled"), "{refused:?}");

    let enabled = sandbox.nomux_ok(&["ext", "enable", "test.crash-at-start", "--json"]);
    assert_eq!(enabled.json()["extension"]["state"], "stopped");
    assert_eq!(enabled.json()["extension"]["reason"], Value::Null);
}

#[test]
fn a_disabled_extension_stops_and_refuses_calls_until_the_user_enables_it() {
    let sandbox = Sandbox::new();
    install(&sandbox, "test.echo");
    let pid = extension_pid(&sandbox, "test.echo");

    let disabled = sandbox.nomux_ok(&["ext", "disable", "test.echo", "--json"]);

    assert_eq!(disabled.json()["extension"]["state"], "disabled");
    assert_eq!(
        disabled.json()["extension"]["reason"],
        "the user disabled the extension"
    );
    assert!(!process_runs(pid));
    let refused = call(&sandbox, "test.echo", "echo", "no");
    assert_eq!(refused.code, ERROR, "{refused:?}");

    let _ = sandbox.nomux_ok(&["ext", "enable", "test.echo"]);
    assert_eq!(
        text_result(&call(&sandbox, "test.echo", "echo", "yes")),
        "yes"
    );
}

#[test]
fn a_client_attaches_to_a_session_while_a_call_of_an_extension_that_hangs_waits() {
    let sandbox = Sandbox::new();
    install(&sandbox, "test.hang");
    let session = new_session(&sandbox, &["sh"]);
    // The method holds the standard output of the extension: the extension
    // answers nothing, also no health request.
    let waiting_call = sandbox.nomux_in_terminal(&["ext", "call", "test.hang", "hang"]);
    wait_for("the extension runs", || {
        (state(&sandbox, "test.hang") == "running").then_some(())
    });

    let mut client = sandbox.nomux_in_terminal(&["attach", &session]);
    client.wait_for_text("$");
    client.type_keys(b"echo nomux-$((40 + 2))\r");
    client.wait_for_text("nomux-42");

    // The attach did not wait for the extension: the call still waits, and
    // the host did not end the extension yet.
    assert!(process_runs(waiting_call.pid()));
    assert_eq!(state(&sandbox, "test.hang"), "running");
}

#[test]
fn a_stop_of_the_daemon_stops_the_extensions_and_no_session() {
    let sandbox = Sandbox::new();
    let session = new_session(&sandbox, &["sleep", "300"]);
    install(&sandbox, "test.echo");
    let pid = extension_pid(&sandbox, "test.echo");

    let _ = sandbox.nomux_ok(&["daemon", "stop"]);

    // The daemon waits for the end of the extension before it ends.
    assert!(!process_runs(pid));
    // The next daemon finds the session and the installed extension, and it
    // starts no extension.
    assert_eq!(sandbox.sessions()[0]["id"], session.as_str());
    assert_eq!(state(&sandbox, "test.echo"), "stopped");
}

#[test]
fn validate_reports_the_field_that_is_not_valid_without_a_daemon() {
    let sandbox = Sandbox::new();
    let package = package(&sandbox, "test.git");
    let valid = sandbox
        .nomux_ok(&["ext", "validate", &package, "--json"])
        .json();
    assert_eq!(valid["manifest"]["valid"], true);
    assert_eq!(valid["manifest"]["id"], "test.git");
    assert_eq!(
        valid["manifest"]["permissions"]["scopes"],
        json!(["sessions:read"])
    );

    let manifest = PathBuf::from(&package).join("extension.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, text.replace("\"test.git\"", "\"Git\"")).unwrap();
    let not_valid = sandbox.nomux(&["ext", "validate", &package, "--json"]);

    assert_eq!(not_valid.code, ERROR, "{not_valid:?}");
    let document = not_valid.json();
    assert_eq!(document["manifest"]["valid"], false);
    assert_eq!(document["manifest"]["problems"][0]["path"], "extension.id");
    let text_form = sandbox.nomux(&["ext", "validate", &package]);
    assert_eq!(text_form.code, ERROR, "{text_form:?}");
    assert!(text_form.stdout.contains("`extension.id`"), "{text_form:?}");

    let missing = sandbox.nomux(&["ext", "validate", "/nomux/no/such/package"]);
    assert_eq!(missing.code, NOT_FOUND, "{missing:?}");
    // The command started no daemon.
    let status = sandbox.nomux(&["daemon", "status"]);
    assert_eq!(status.code, NOT_FOUND, "{status:?}");
}
