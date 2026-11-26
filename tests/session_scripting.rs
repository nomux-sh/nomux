//! The commands for scripts and the shortcuts, with the real binaries:
//! `send` and `capture`, `kill --all` and its question, `attach -d` and
//! `attach -r`, and bare `nomux` on a terminal.

use nomux_end_to_end::{Sandbox, wait_for};

fn new_shell(sandbox: &Sandbox, name: &str) -> String {
    sandbox
        .nomux_ok(&["new", "-q", "-n", name, "--", "sh"])
        .stdout
        .trim()
        .to_owned()
}

fn running_sessions(sandbox: &Sandbox) -> usize {
    sandbox
        .sessions()
        .iter()
        .filter(|session| session["state"] == "running")
        .count()
}

#[test]
fn send_types_a_line_into_a_session_and_capture_reads_its_output() {
    let sandbox = Sandbox::new();
    let _ = new_shell(&sandbox, "api");

    let sent = sandbox.nomux_ok(&["send", "api", "echo", "sent-$((6", "*", "7))"]);
    assert_eq!(sent.stdout, "");
    wait_for("the shell ran the line", || {
        let output = sandbox.nomux_ok(&["capture", "api"]).stdout;
        output.contains("sent-42").then_some(())
    });
    // Without Enter the text waits on the line; -k sends the Enter key.
    let _ = sandbox.nomux_ok(&["send", "--no-enter", "api", "echo later"]);
    let _ = sandbox.nomux_ok(&["send", "-k", "api", "Enter"]);
    wait_for("the shell ran the second line", || {
        let output = sandbox.nomux_ok(&["capture", "api"]).stdout;
        // A line of the output, not the echo of the typed line.
        let ran = output
            .lines()
            .any(|line| line.trim_end().ends_with("later") && !line.contains("echo"));
        ran.then_some(())
    });
    assert_eq!(sandbox.nomux(&["send", "api", "two\nlines"]).code, 1);
    assert_eq!(sandbox.nomux(&["send", "-k", "api", "NoKey"]).code, 1);
    assert_eq!(sandbox.nomux(&["send", "nothing", "x"]).code, 2);
}

#[test]
fn kill_all_asks_on_a_terminal_and_needs_yes_without_one() {
    let sandbox = Sandbox::new();
    let _ = new_shell(&sandbox, "one");
    let _ = new_shell(&sandbox, "two");

    let mut terminal = sandbox.nomux_in_terminal(&["kill", "--all"]);
    terminal.wait_for_text("end 2 sessions? [y/N]");
    terminal.type_keys(b"n\r");
    assert_eq!(terminal.wait_for_exit(), 1, "{}", terminal.text_output());
    assert_eq!(running_sessions(&sandbox), 2);

    let without_terminal = sandbox.nomux(&["kill", "--all"]);
    assert_eq!(without_terminal.code, 1, "{without_terminal:?}");
    assert!(
        without_terminal.stderr.contains("--yes"),
        "{without_terminal:?}"
    );
    let dry_run = sandbox.nomux_ok(&["kill", "--all", "--dry-run"]).stdout;
    assert_eq!(dry_run.lines().count(), 2, "{dry_run}");
    assert_eq!(running_sessions(&sandbox), 2);

    let mut terminal = sandbox.nomux_in_terminal(&["kill", "--all"]);
    terminal.wait_for_text("[y/N]");
    terminal.type_keys(b"y\r");
    assert_eq!(terminal.wait_for_exit(), 0, "{}", terminal.text_output());
    assert_eq!(running_sessions(&sandbox), 0);
    assert_eq!(
        sandbox.nomux_ok(&["kill", "--all", "--yes"]).stdout,
        "no session runs\n"
    );
}

#[test]
fn attach_detaches_the_others_or_only_watches() {
    let sandbox = Sandbox::new();
    let id = new_shell(&sandbox, "work");

    let mut first = sandbox.nomux_in_terminal(&["attach", "work"]);
    first.wait_for_text("$");
    let mut second = sandbox.nomux_in_terminal(&["attach", "-d", "work"]);
    assert_eq!(first.wait_for_exit(), 0, "{}", first.text_output());
    second.wait_for_text("$");

    // A read-only client sends no key: the shell does not get the line.
    let mut watcher = sandbox.nomux_in_terminal(&["attach", "-r", &id]);
    watcher.wait_for_text("$");
    watcher.type_keys(b"echo from-watcher\r");
    second.type_keys(b"echo from-second\r");
    watcher.wait_for_text("from-second");
    let output = sandbox.nomux_ok(&["capture", &id]).stdout;
    assert!(!output.contains("from-watcher"), "{output}");
    // Its detach keys work.
    watcher.type_keys(b"\x1cd");
    assert_eq!(watcher.wait_for_exit(), 0, "{}", watcher.text_output());
    second.type_keys(b"\x1cd");
    assert_eq!(second.wait_for_exit(), 0, "{}", second.text_output());
}

#[test]
fn bare_nomux_on_a_terminal_starts_a_session_and_then_attaches_to_it() {
    let sandbox = Sandbox::new();

    let mut terminal = sandbox.nomux_in_terminal(&[]);
    terminal.wait_for_text("$");
    terminal.type_keys(b"\x1cd");
    assert_eq!(terminal.wait_for_exit(), 0, "{}", terminal.text_output());
    let sessions = sandbox.sessions();
    assert_eq!(sessions.len(), 1, "{sessions:?}");

    let mut terminal = sandbox.nomux_in_terminal(&[]);
    terminal.wait_for_text("$");
    terminal.type_keys(b"\x1cd");
    assert_eq!(terminal.wait_for_exit(), 0, "{}", terminal.text_output());
    assert_eq!(sandbox.sessions().len(), 1);

    // Without a terminal, bare `nomux` prints the help.
    let piped = sandbox.nomux(&[]);
    assert_eq!(piped.code, 1);
    assert!(piped.stderr.contains("Commands:"), "{piped:?}");
}
