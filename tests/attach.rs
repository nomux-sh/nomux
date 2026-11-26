//! `nomux attach` with the real binaries and a pseudoterminal: the screen,
//! the keys, the window size, detach, more than one client, the end of the
//! daemon, a flood of output and the end of the session.

use std::fs;

use nomux_end_to_end::{
    Sandbox, TerminalClient, kill_process, process_runs, resident_memory_kib, wait_for,
};
use nomux_term::{Attributes, Color};

/// `Ctrl-\` and then `d`.
const DETACH_KEYS: &[u8] = b"\x1cd";

/// The sequence that the CLI writes before each snapshot of a screen.
const SNAPSHOT_START: &str = "\x1b[?1049l\x1b[!p";

/// Starts a session with `command` and returns its identifier.
fn new_session(sandbox: &Sandbox, command: &[&str]) -> String {
    let arguments = [&["new", "--quiet", "--"], command].concat();
    sandbox.nomux_ok(&arguments).stdout.trim().to_owned()
}

fn session_ids(sandbox: &Sandbox) -> Vec<String> {
    let listed = sandbox.nomux_ok(&["ls", "--quiet"]).stdout;
    listed.lines().map(str::to_owned).collect()
}

/// Attaches to a session that runs a shell and waits for its prompt.
fn attach_to_shell(sandbox: &Sandbox, id: &str) -> TerminalClient {
    let mut client = sandbox.nomux_in_terminal(&["attach", id]);
    client.wait_for_text("$");
    client
}

#[test]
fn keys_reach_the_program_and_a_second_attach_shows_the_same_screen() {
    let sandbox = Sandbox::new();
    let id = new_session(&sandbox, &["sh"]);

    let mut first = attach_to_shell(&sandbox, &id);
    assert!(first.is_in_raw_mode());
    first.type_keys(b"echo nomux-$((40 + 2))\r");
    // The second prompt comes after the output of the command.
    let screen = first.wait_for_rows("the output and the next prompt", |rows| {
        rows.iter().any(|row| row == "nomux-42")
            && rows.iter().filter(|row| row.contains('$')).count() == 2
    });

    first.type_keys(DETACH_KEYS);
    assert_eq!(first.wait_for_exit(), 0, "{}", first.text_output());
    assert!(
        first
            .text_output()
            .contains(&format!("detached from session {id}")),
        "{}",
        first.text_output()
    );
    assert!(!first.is_in_raw_mode());
    assert_eq!(session_ids(&sandbox), std::slice::from_ref(&id));

    // The command wrote nothing after the detach: the screen of the second
    // client comes from the snapshot only.
    let mut second = sandbox.nomux_in_terminal(&["attach", &id]);
    second.wait_for_rows("the screen of the first client", |rows| rows == screen);
}

#[test]
fn a_client_gets_the_screen_of_a_full_screen_program_that_drew_before_the_attach() {
    let sandbox = Sandbox::new();
    // The alternate screen, a red and bold word, and text at a position.
    let draw = "printf '\\033[?1049h\\033[2J\\033[3;5H\\033[1;31mFULL\\033[0m\\033[10;1Hline ten'; \
                touch drawn; exec cat";
    let id = new_session(&sandbox, &["sh", "-c", draw]);
    wait_for("the program drew its screen", || {
        sandbox.home().join("drawn").exists().then_some(())
    });

    let mut client = sandbox.nomux_in_terminal(&["attach", &id]);
    let rows = client.wait_for_text("line ten");

    assert_eq!(rows[2], "    FULL");
    assert_eq!(rows[9], "line ten");
    assert_eq!(rows.iter().filter(|row| !row.is_empty()).count(), 2);
    let style = client.style_at(2, 4);
    assert_eq!(style.foreground, Color::Indexed(1));
    assert!(style.attributes.contains(Attributes::BOLD));
    assert_eq!(client.style_at(9, 0).attributes, Attributes::empty());
}

#[test]
fn the_session_gets_the_window_size_of_the_client_and_each_change_of_it() {
    let sandbox = Sandbox::new();
    // The command prints the size of its terminal at each change. The keys
    // and the window size use two connections, thus a test that types a
    // command after a resize does not know which of the two arrives first.
    let report = "trap 'stty size' WINCH; touch ready; while :; do sleep 0.1; done";
    let id = new_session(&sandbox, &["sh", "-c", report]);
    // A program without a handler for SIGWINCH ignores the signal. The
    // signal of the attach must not arrive before the trap.
    wait_for("the command set its trap", || {
        sandbox.home().join("ready").exists().then_some(())
    });

    // A new session has 24 rows and 80 columns.
    let mut client = sandbox.nomux_in_window(&["attach", &id], 20, 70);
    client.wait_for_rows("the size of the attach", |rows| {
        rows.iter().any(|row| row == "20 70")
    });

    client.resize(30, 100);
    client.wait_for_rows("the new size", |rows| {
        rows.iter().any(|row| row == "30 100")
    });
}

#[test]
fn two_clients_show_the_same_session_and_a_detach_request_detaches_the_two() {
    let sandbox = Sandbox::new();
    let id = new_session(&sandbox, &["sh"]);
    let mut first = attach_to_shell(&sandbox, &id);
    let mut second = attach_to_shell(&sandbox, &id);

    first.type_keys(b"echo from-$((1))\r");
    second.wait_for_text("from-1");
    second.type_keys(b"echo from-$((2))\r");
    let screen = first.wait_for_rows("the output of the two commands", |rows| {
        rows.iter().any(|row| row == "from-2")
            && rows.iter().filter(|row| row.contains('$')).count() == 3
    });
    second.wait_for_rows("the screen of the first client", |rows| rows == screen);

    let detached = sandbox.nomux_ok(&["detach", &id, "--json"]).json();
    assert_eq!(detached["detached"]["id"], id.as_str());
    assert_eq!(detached["detached"]["clients"], 2);
    for client in [&mut first, &mut second] {
        assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());
        assert!(client.text_output().contains("detached from session"));
        assert!(!client.is_in_raw_mode());
    }

    assert_eq!(session_ids(&sandbox), std::slice::from_ref(&id));
    let again = sandbox.nomux_ok(&["detach", &id]);
    assert_eq!(
        again.stdout,
        format!("detached 0 clients from session {id}\n")
    );
    assert_eq!(sandbox.nomux(&["detach", "no-session"]).code, 2);
}

#[test]
fn a_client_that_leaves_changes_nothing_for_the_other_client() {
    let sandbox = Sandbox::new();
    let id = new_session(&sandbox, &["sh"]);
    let mut stays = attach_to_shell(&sandbox, &id);
    let leaves = attach_to_shell(&sandbox, &id);

    // The process of the client ends without a detach.
    kill_process(leaves.pid());
    wait_for("the client is gone", || {
        (!process_runs(leaves.pid())).then_some(())
    });

    stays.type_keys(b"echo still-$((1))\r");
    stays.wait_for_text("still-1");
    assert_eq!(session_ids(&sandbox), [id]);
}

#[test]
fn the_prefix_keys_detach_in_each_key_form_and_the_setting_chooses_them() {
    let mut sandbox = Sandbox::new();
    let id = new_session(&sandbox, &["sh"]);

    // Ctrl-a, the default prefix, and then d.
    let mut client = attach_to_shell(&sandbox, &id);
    client.type_keys(b"\x01d");
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());
    // Ctrl-a in the form of the kitty keyboard protocol, which a program of
    // the session can turn on, and then d in the same form.
    let mut client = attach_to_shell(&sandbox, &id);
    client.type_keys(b"\x1b[97;5u\x1b[100u");
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());

    // With the prefix Ctrl-b, Ctrl-a and d reach the program.
    sandbox.set_variable("NOMUX_PREFIX", "C-b");
    let mut client = attach_to_shell(&sandbox, &id);
    client.type_keys(b"cat -v\r");
    client.type_keys(b"\x01d\r");
    client.wait_for_text("^Ad");
    client.type_keys(b"\x04\x02d");
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());
    // The option comes before the variable.
    let mut client = sandbox.nomux_in_terminal(&["attach", &id, "--prefix", "C-]"]);
    client.wait_for_text("$");
    client.type_keys(b"\x1dd");
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());
}

#[test]
fn the_end_of_the_session_ends_the_attach_with_the_exit_status() {
    let sandbox = Sandbox::new();

    let exits = new_session(&sandbox, &["sh"]);
    let mut client = attach_to_shell(&sandbox, &exits);
    client.type_keys(b"echo last-$((1)); exit 7\r");
    assert_eq!(client.wait_for_exit(), 7, "{}", client.text_output());
    let output = client.text_output();
    // The output of the command comes before the end of the attach.
    assert!(output.contains("last-1"), "{output}");
    assert!(
        output.contains(&format!("session {exits} ended with exit status 7")),
        "{output}"
    );
    assert!(!client.is_in_raw_mode());

    let killed = new_session(&sandbox, &["sh"]);
    let mut client = sandbox.nomux_in_terminal(&["attach", &killed, "--json"]);
    client.wait_for_text("$");
    client.type_keys(b"kill -9 $$\r");
    assert_eq!(client.wait_for_exit(), 128 + 9, "{}", client.text_output());
    let document = format!(
        "{{\"schema_version\":1,\"attach\":{{\"id\":\"{killed}\",\"name\":null,\"end\":\"signaled\",\"signal\":9}}}}"
    );
    assert!(
        client.text_output().contains(&document),
        "{}",
        client.text_output()
    );

    assert_eq!(session_ids(&sandbox), [] as [String; 0]);
}

/// The sequence of the issue of the first CLI commands: new, attach, end of
/// the daemon, attach again, end of the session.
#[test]
fn sigkill_of_the_daemon_ends_the_attach_with_an_error_and_a_new_attach_works() {
    let sandbox = Sandbox::new();
    let id = new_session(&sandbox, &["sh"]);
    let mut client = attach_to_shell(&sandbox, &id);
    client.type_keys(b"echo before-$((1))\r");
    let screen = client.wait_for_rows("the output and the next prompt", |rows| {
        rows.iter().any(|row| row == "before-1")
            && rows.iter().filter(|row| row.contains('$')).count() == 2
    });

    let daemon = sandbox.daemon_pid();
    kill_process(daemon);

    assert_eq!(client.wait_for_exit(), 1, "{}", client.text_output());
    let output = client.text_output();
    assert!(
        output.contains(&format!("The session continues: run `nomux attach {id}`")),
        "{output}"
    );
    assert!(!client.is_in_raw_mode());

    // The attach starts a new daemon, which finds the session.
    let mut client = sandbox.nomux_in_terminal(&["attach", &id]);
    client.wait_for_rows("the screen from before the end of the daemon", |rows| {
        rows == screen
    });
    assert_ne!(sandbox.daemon_pid(), daemon);

    // `kill` ends the command with SIGHUP, which has the number 1.
    assert_eq!(
        sandbox.nomux_ok(&["kill", &id]).stdout,
        format!("session {id} ended\n")
    );
    assert_eq!(client.wait_for_exit(), 128 + 1, "{}", client.text_output());
    assert!(
        client
            .text_output()
            .contains(&format!("session {id} ended by signal 1")),
        "{}",
        client.text_output()
    );
}

#[test]
fn a_flood_for_a_slow_client_uses_no_more_daemon_memory_and_ends_with_the_correct_screen() {
    const FLOOD_BYTES: u64 = 32 * 1024 * 1024;
    // Much smaller than the flood, and larger than the buffers of an attach.
    const PERMITTED_GROWTH_KIB: u64 = 8 * 1024;

    let sandbox = Sandbox::new();
    let flood = format!(
        "read start; yes 'a line of the flood of output' | head -c {FLOOD_BYTES}; touch flooded; \
         printf '\\033[2J\\033[Hafter the flood\\r\\nsecond row'; exec cat"
    );
    let id = new_session(&sandbox, &["sh", "-c", &flood]);
    let mut client = sandbox.nomux_in_terminal(&["attach", &id]);
    client.type_keys(b"go\r");
    client.wait_for_text("a line of the flood");
    let daemon = sandbox.daemon_pid();
    let memory_before = resident_memory_kib(daemon);

    // The terminal takes no more output, and the command writes all of its
    // flood: the holder does not wait for a client.
    client.pause_reading();
    wait_for("the command wrote the flood", || {
        sandbox.home().join("flooded").exists().then_some(())
    });
    let growth = resident_memory_kib(daemon).saturating_sub(memory_before);
    assert!(
        growth < PERMITTED_GROWTH_KIB,
        "the daemon grew by {growth} KiB during a flood of {FLOOD_BYTES} bytes"
    );

    client.resume_reading();
    let rows = client.wait_for_rows("the screen after the flood", |rows| {
        rows[0] == "after the flood" && rows[1] == "second row"
    });
    assert!(rows[2..].iter().all(String::is_empty), "{rows:#?}");
    // The client did not get each byte: it got the screen again.
    let snapshots = client.text_output().matches(SNAPSHOT_START).count();
    assert!(snapshots >= 2, "the client got {snapshots} snapshot");
}

#[test]
fn new_with_attach_shows_the_new_session_and_gives_its_exit_status() {
    let sandbox = Sandbox::new();
    let command = "printf 'attached at the start'; read line; exit 3";

    let mut client = sandbox.nomux_in_terminal(&["new", "--attach", "--", "sh", "-c", command]);
    client.wait_for_text("attached at the start");
    client.type_keys(b"\r");

    assert_eq!(client.wait_for_exit(), 3, "{}", client.text_output());
    assert!(client.text_output().contains("ended with exit status 3"));
}

#[test]
fn attach_without_a_terminal_and_to_no_session_is_an_error() {
    let sandbox = Sandbox::new();

    let no_terminal = sandbox.nomux(&["new", "--attach", "--", "sh"]);
    assert_eq!(no_terminal.code, 1, "{no_terminal:?}");
    assert!(
        no_terminal.stderr.contains("needs a terminal"),
        "{no_terminal:?}"
    );
    assert!(
        !sandbox.runtime_directory().exists(),
        "a session that no terminal can show must not start"
    );

    let mut client = sandbox.nomux_in_terminal(&["attach", "no-session"]);
    assert_eq!(client.wait_for_exit(), 2, "{}", client.text_output());
    assert!(
        client
            .text_output()
            .contains("no session has the identifier")
    );
    assert!(!client.is_in_raw_mode());
}

/// An option of `nomux` before `--` is not an argument of the command.
#[test]
fn the_command_after_the_separator_does_not_take_the_options_of_nomux() {
    let sandbox = Sandbox::new();
    let marker = sandbox.home().join("arguments");

    let created = sandbox
        .nomux_ok(&[
            "new",
            "--json",
            "--",
            "sh",
            "-c",
            "echo \"$@\" > arguments; sleep 600",
            "sh",
            "--quiet",
        ])
        .json();
    let id = created["session"]["id"].as_str().unwrap();

    // The command got its own arguments, also one that `nomux` knows.
    let arguments = wait_for("the command wrote its arguments", || {
        fs::read_to_string(&marker)
            .ok()
            .filter(|text| !text.is_empty())
    });
    assert_eq!(arguments, "--quiet\n");
    assert_eq!(session_ids(&sandbox), [id.to_owned()]);
}
