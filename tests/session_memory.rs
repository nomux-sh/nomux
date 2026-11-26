//! The memory of each session: `nomux ls --memory` gives the memory of the
//! process tree of the command of a session, and the memory of its holder
//! as a separate value.

use nomux_end_to_end::{Sandbox, kill_process, wait_for};
use serde_json::Value;

const MEMORY_USER: &str = env!("CARGO_BIN_EXE_nomux-test-memory-user");
const MEBIBYTE: u64 = 1024 * 1024;
/// What a process uses for its program and its libraries, and the pages
/// that the allocator keeps: the largest value that the command of a test
/// can be above the memory that it asked for.
const TOLERANCE: u64 = 16 * MEBIBYTE;
/// A holder uses approximately 2 MiB.
const HOLDER_LIMIT: u64 = 16 * MEBIBYTE;

/// Starts a session whose command allocates the first number of mebibytes
/// and starts a child for the next number. Returns when each process of
/// the chain has its memory.
fn new_session_that_uses(sandbox: &Sandbox, name: &str, mebibytes: &[&str]) -> String {
    let ready_file = sandbox.home().join(name);
    let ready_text = ready_file.to_str().unwrap();
    let command = [
        &["new", "--quiet", "--", MEMORY_USER, ready_text],
        mebibytes,
    ]
    .concat();
    let id = sandbox.nomux_ok(&command).stdout.trim().to_owned();
    wait_for("the command has its memory", || {
        ready_file.exists().then_some(())
    });
    id
}

fn measured(sandbox: &Sandbox) -> Value {
    sandbox.nomux_ok(&["ls", "--memory", "--json"]).json()
}

fn bytes(value: &Value) -> u64 {
    value
        .as_u64()
        .unwrap_or_else(|| panic!("not a number of bytes: {value}"))
}

#[test]
fn the_memory_of_a_session_is_the_memory_of_its_command_and_of_each_child_of_the_command() {
    let sandbox = Sandbox::new();
    // The command uses 48 MiB, and its child uses 24 MiB.
    let id = new_session_that_uses(&sandbox, "ready", &["48", "24"]);

    let document = measured(&sandbox);

    let session = &document["sessions"][0];
    assert_eq!(session["id"], id.as_str());
    let memory = &session["memory"];
    assert_eq!(memory["command_processes"], 2, "{document}");
    // The command alone has less than 72 MiB: the value has the child.
    let command_bytes = bytes(&memory["command_bytes"]);
    assert!(
        (72 * MEBIBYTE..72 * MEBIBYTE + TOLERANCE).contains(&command_bytes),
        "{document}"
    );
    // The holder is not in the value of the command.
    let holder_bytes = bytes(&memory["holder_bytes"]);
    assert!((1..HOLDER_LIMIT).contains(&holder_bytes), "{document}");
    let measure = if cfg!(target_os = "linux") {
        "pss"
    } else {
        "rss"
    };
    assert_eq!(memory["measure"], measure);
    assert_eq!(
        document["memory_total"],
        serde_json::json!({
            "command_bytes": command_bytes,
            "command_processes": 2,
            "holder_bytes": holder_bytes,
            "sessions": 1,
            "measure": measure,
        })
    );

    // The text has the same session, with the measure and a total line.
    let text = sandbox.nomux_ok(&["ls", "--memory"]).stdout;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].starts_with("ID "), "{text}");
    assert!(lines[1].starts_with(&id), "{text}");
    assert!(lines[1].contains(" 2  "), "{text}");
    assert!(lines[1].contains(measure), "{text}");
    assert!(lines[2].starts_with("total: "), "{text}");
    assert!(lines[2].contains("in 2 processes of 1 session"), "{text}");
}

#[test]
fn a_session_without_a_process_has_no_memory_and_the_total_has_only_the_other_sessions() {
    let sandbox = Sandbox::new();
    let first = new_session_that_uses(&sandbox, "first", &["8"]);
    let second = new_session_that_uses(&sandbox, "second", &["8"]);
    // Without its holder, the second session is dormant.
    kill_process(sandbox.holder_pid(&second));
    wait_for("the session is dormant", || {
        sandbox
            .sessions()
            .iter()
            .any(|session| session["id"] == second.as_str() && session["state"] == "dormant")
            .then_some(())
    });

    let document = measured(&sandbox);

    let sessions = document["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{document}");
    let of = |id: &str| sessions.iter().find(|session| session["id"] == id).unwrap();
    assert_eq!(of(&second)["memory"], Value::Null, "{document}");
    let memory = &of(&first)["memory"];
    assert_eq!(memory["command_processes"], 1, "{document}");
    assert_eq!(document["memory_total"]["sessions"], 1);
    assert_eq!(
        document["memory_total"]["command_bytes"],
        memory["command_bytes"]
    );
    // Without `--memory`, the document has no field of the memory.
    let plain = sandbox.nomux_ok(&["ls", "--json"]).json();
    assert_eq!(plain["sessions"][0].get("memory"), None);
    assert_eq!(plain.get("memory_total"), None);
}
