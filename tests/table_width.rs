//! The tables of `nomux ls` and `nomux status` on a terminal and in a pipe:
//! on a terminal, a long command is shortened and the state and the message
//! stay visible; in a pipe and with `--json`, the command is complete.

use nomux_end_to_end::{Sandbox, wait_for};

const COLUMNS: u16 = 60;
const LONG_COMMAND: &str =
    "echo a long command line with many words to make the table wide; sleep 600";

fn new_long_session(sandbox: &Sandbox) -> String {
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh", "-c", LONG_COMMAND])
        .stdout
        .trim()
        .to_owned();
    wait_for("the session has a status", || {
        let tree = sandbox.nomux_ok(&["status", "--tree", "--json"]).json();
        tree["status"]["sessions"]
            .as_array()?
            .iter()
            .any(|run| run["id"] == id.as_str())
            .then_some(())
    });
    id
}

/// The lines that `nomux <arguments>` writes on a terminal of 60 columns.
fn lines_on_terminal(sandbox: &Sandbox, arguments: &[&str]) -> Vec<String> {
    let mut client = sandbox.nomux_in_window(arguments, 24, COLUMNS);
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());
    client
        .text_output()
        .lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect()
}

#[test]
fn a_terminal_gets_a_table_that_fits_and_a_pipe_gets_the_full_command() {
    let mut sandbox = Sandbox::new();
    // The lines have no colour: each character has one column.
    sandbox.set_variable("NO_COLOR", "1");
    let id = new_long_session(&sandbox);

    for (arguments, state) in [(&["ls"][..], "running"), (&["status"], "idle")] {
        let lines = lines_on_terminal(&sandbox, arguments);
        let row = lines
            .iter()
            .find(|line| line.contains(&id))
            .unwrap_or_else(|| panic!("{arguments:?}: no row of {id} in {lines:?}"));
        assert!(row.contains('\u{2026}'), "{arguments:?}: {row}");
        assert!(row.contains(state), "{arguments:?}: {row}");
        for line in &lines {
            assert!(
                line.chars().count() <= usize::from(COLUMNS),
                "{arguments:?}: {line:?}"
            );
        }

        let piped = sandbox.nomux_ok(arguments).stdout;
        assert!(piped.contains(LONG_COMMAND), "{arguments:?}: {piped}");
    }
    let document = sandbox.nomux_ok(&["ls", "--json"]).json();
    assert_eq!(document["sessions"][0]["command"][2], LONG_COMMAND);
}
