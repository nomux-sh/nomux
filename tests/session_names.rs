//! Session names with the real binaries: a name stays with its session
//! after a restart of the host, a target finds a session by its name, and
//! two clients that attach to a name or create it at the same time get one
//! session.

use std::thread;

use nomux_end_to_end::{Sandbox, process_runs, wait_for};
use serde_json::Value;

/// The sessions that are not ended, as `(name, id, state)`.
fn named_sessions(sandbox: &Sandbox) -> Vec<(Value, String, String)> {
    sandbox
        .sessions()
        .into_iter()
        .map(|session| {
            (
                session["name"].clone(),
                session["id"].as_str().unwrap().to_owned(),
                session["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_name_stays_with_its_session_after_a_restart_of_the_host() {
    let sandbox = Sandbox::new();
    let created = sandbox
        .nomux_ok(&["new", "--json", "-n", "api", "--", "sh"])
        .json();
    let id = created["session"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["session"]["name"], "api");
    let pid = u32::try_from(created["session"]["pid"].as_u64().unwrap()).unwrap();
    let _ = sandbox.nomux_ok(&["rename", "api", "web"]);

    sandbox.kill_each_process();
    wait_for("the command of the session is gone", || {
        (!process_runs(pid)).then_some(())
    });

    // A new daemon reads the name from the state directory.
    assert_eq!(
        named_sessions(&sandbox),
        [(Value::from("web"), id.clone(), "dormant".to_owned())]
    );
    // The dormant session keeps its name for itself.
    let taken = sandbox.nomux(&["new", "-n", "web"]);
    assert_eq!(taken.code, 1, "{taken:?}");
    assert!(
        taken.stderr.contains(&format!("in use by session {id}")),
        "{taken:?}"
    );
    // The name is a target: the session starts again by its name.
    let started = sandbox.nomux_ok(&["start", "web", "--json"]).json();
    assert_eq!(started["session"]["id"], id.as_str());
    assert_eq!(started["session"]["state"], "running");
    let _ = sandbox.nomux_ok(&["kill", "web"]);
    assert_eq!(named_sessions(&sandbox), []);
}

#[test]
fn two_clients_that_attach_to_a_name_or_create_it_at_once_get_one_session() {
    let sandbox = Sandbox::new();
    let sandbox = &sandbox;
    let ends: Vec<i32> = thread::scope(|scope| {
        let clients: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(move || {
                    let mut terminal = sandbox.nomux_in_terminal(&["new", "-A", "-n", "work"]);
                    terminal.wait_for_text("$");
                    terminal.type_keys(b"\x1cd");
                    terminal.wait_for_exit()
                })
            })
            .collect();
        clients
            .into_iter()
            .map(|client| client.join().unwrap())
            .collect()
    });
    assert_eq!(ends, [0, 0]);
    let sessions = named_sessions(sandbox);
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(sessions[0].0, "work");
}
