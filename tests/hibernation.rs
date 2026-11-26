//! The hibernation of an idle agent session, with the real binaries and the
//! test adapter: a trigger stops the agent, the session keeps its output,
//! and an attach wakes it with the resume command of the adapter.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use nomux_end_to_end::{Sandbox, adapter_package, binary, process_runs, wait_for};
use serde_json::Value;

const RESTORED_MARK: &str = "nomux: the session started again";

/// Writes the made-up agent tool `testtool` into `bin` of the sandbox and
/// puts `bin` first in `PATH`. `testtool <name>` starts a conversation,
/// `testtool --resume <name>` continues it. The tool starts a child, writes
/// the process ID of the child to `<name>.child`, reports its work through
/// the hooks of the test adapter, and then waits.
///
/// A conversation whose name starts with `broken` reports a resume command
/// whose program does not exist.
///
/// The daemon takes the token of a session from its holder after the start,
/// and it refuses a hook before that. Thus the tool waits a moment before
/// its first hook.
fn install_tool(sandbox: &mut Sandbox) {
    let bin = sandbox.home().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let nomux = binary("nomux");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = --resume ]; then name=$2; echo "testtool resumed $name"; else name=$1; echo "testtool started $name"; fi
sleep 300 &
echo $! > "$HOME/$name.child"
sleep 0.5
case $name in broken*) resume=',"resume":"missing-tool --resume '"$name"'"' ;; esac
printf '{{"session":"%s"%s}}' "$name" "$resume" | {nomux} hook testtool prompt
printf '{{}}' | {nomux} hook testtool stop
exec cat
"#,
        nomux = nomux.display()
    );
    let tool = bin.join("testtool");
    fs::write(&tool, script).unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    sandbox.set_variable("PATH", format!("{}:/usr/bin:/bin", bin.display()));
}

/// A sandbox with `configuration`, the test adapter and `testtool`.
fn sandbox_with_agents(configuration: &str) -> Sandbox {
    let mut sandbox = Sandbox::new();
    sandbox.write_configuration(configuration);
    install_tool(&mut sandbox);
    let package = adapter_package(&sandbox.home().join("packages"));
    let _ = sandbox.nomux_ok(&["ext", "install", package.to_str().unwrap(), "--grant-all"]);
    sandbox
}

/// Starts `testtool <name>` in a new session and waits until its adapter
/// reports the work as done. Returns the session identifier and the process
/// IDs of the command and of its child.
fn start_agent(sandbox: &Sandbox, name: &str) -> (String, u32, u32) {
    let created = sandbox
        .nomux_ok(&["new", "--json", "--", "testtool", name])
        .json();
    let id = created["session"]["id"].as_str().unwrap().to_owned();
    let pid = u32::try_from(created["session"]["pid"].as_u64().unwrap()).unwrap();
    wait_for("the agent reports that it is done", || {
        let explained = sandbox
            .nomux_ok(&["agent", "explain", &id, "--json"])
            .json();
        (explained["explain"]["run"]["status"]["state"] == "done").then_some(())
    });
    let child_file = sandbox.home().join(format!("{name}.child"));
    let child = wait_for("the agent wrote the process ID of its child", || {
        fs::read_to_string(&child_file).ok()?.trim().parse().ok()
    });
    (id, pid, child)
}

fn listed(sandbox: &Sandbox, id: &str) -> Value {
    sandbox
        .sessions()
        .into_iter()
        .find(|session| session["id"] == id)
        .unwrap_or_else(|| panic!("session {id} is in the list"))
}

fn is_hibernated(session: &Value) -> bool {
    session["state"] == "dormant" && session["hibernation"]["woken_at"].is_null()
}

fn wait_until_hibernated(sandbox: &Sandbox, id: &str) -> Value {
    wait_for("the session hibernates", || {
        Some(listed(sandbox, id)).filter(is_hibernated)
    })
}

#[test]
fn an_idle_agent_hibernates_by_count_and_wakes_on_attach_with_its_resume_command() {
    let sandbox = sandbox_with_agents(
        "[agents]\nmax_live = 1\nhibernate_after = \"1s\"\nhibernate_on_pressure = false\n",
    );
    let (alpha, alpha_pid, alpha_child) = start_agent(&sandbox, "alpha");
    let (beta, ..) = start_agent(&sandbox, "beta");

    // Two live agents, one at most: alpha is idle for the longest time.
    let hibernated = wait_until_hibernated(&sandbox, &alpha);
    let hibernation = &hibernated["hibernation"];
    assert_eq!(hibernation["reason"], "count", "{hibernated}");
    assert_eq!(hibernation["resume_command"], "testtool --resume alpha");
    assert!(
        hibernation["freed_bytes"].as_u64().unwrap() > 0,
        "{hibernated}"
    );
    assert_eq!(listed(&sandbox, &beta)["state"], "running");
    // The process tree of the old run is gone.
    wait_for("the old run ends", || {
        (!process_runs(alpha_pid) && !process_runs(alpha_child)).then_some(())
    });

    let table = sandbox.nomux_ok(&["ls"]).stdout;
    let row = table.lines().find(|line| line.starts_with(&alpha)).unwrap();
    assert!(
        row.contains("hibernated") && row.contains("count"),
        "{table}"
    );
    let scrollback = sandbox.nomux_ok(&["capture", &alpha]).stdout;
    assert!(
        scrollback.contains("testtool started alpha"),
        "{scrollback}"
    );

    // The attach shows the old screen first, then the resumed run.
    let mut terminal = sandbox.nomux_in_window(&["attach", &alpha], 30, 100);
    terminal.wait_for_text("testtool resumed alpha");
    let output = terminal.text_output();
    let old = output
        .find("testtool started alpha")
        .expect("the old screen");
    let mark = output.find(RESTORED_MARK).expect("the mark");
    let resumed = output
        .find("testtool resumed alpha")
        .expect("the resumed run");
    assert!(old < mark && mark < resumed, "{output}");
    terminal.type_keys(b"\x1cd");
    assert_eq!(terminal.wait_for_exit(), 0);

    let woken = listed(&sandbox, &alpha);
    assert_eq!(woken["state"], "running", "{woken}");
    assert!(
        woken["hibernation"]["woken_at"].as_u64().is_some(),
        "{woken}"
    );
    assert_eq!(
        woken["command"],
        serde_json::json!(["testtool", "--resume", "alpha"])
    );
    // The new run started in the working directory of the session.
    assert_ne!(woken["pid"], alpha_pid);
    // On macOS, `/tmp` is a link to `/private/tmp`.
    let cwd = fs::canonicalize(woken["cwd"].as_str().unwrap()).unwrap();
    assert_eq!(cwd, fs::canonicalize(sandbox.home()).unwrap());
}

#[test]
fn a_wake_that_fails_keeps_the_session_hibernated_with_the_reason_and_the_command() {
    let sandbox = sandbox_with_agents("[agents]\nhibernate_on_pressure = false\n");
    let (broken, ..) = start_agent(&sandbox, "broken");

    // A session that no adapter reports cannot hibernate.
    let shell = sandbox.nomux_ok(&["new", "--quiet", "--", "sh"]).stdout;
    let refused = sandbox.nomux(&["hibernate", shell.trim()]);
    assert_eq!(refused.code, 1, "{refused:?}");
    assert!(
        refused
            .stderr
            .contains("no adapter reports it as an agent session"),
        "{refused:?}"
    );

    let hibernated = sandbox.nomux_ok(&["hibernate", &broken]).stdout;
    assert!(hibernated.contains("hibernated"), "{hibernated}");
    let woke = sandbox.nomux(&["wake", &broken]);
    assert_eq!(woke.code, 1, "{woke:?}");
    assert!(
        woke.stderr
            .contains("the program missing-tool is not in PATH")
            && woke.stderr.contains("missing-tool --resume broken"),
        "{woke:?}"
    );

    let session = listed(&sandbox, &broken);
    assert!(is_hibernated(&session), "{session}");
    let error = session["hibernation"]["wake_error"].as_str().unwrap();
    assert!(error.contains("missing-tool"), "{session}");
}
