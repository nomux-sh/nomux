//! The workspace tree with the real binaries: the `project` and `group`
//! commands change it, a second client keeps a copy with the events, a
//! restart of the daemon keeps it, and `nomux status --tree` shows the
//! sessions below their Groups and Projects.

use std::fs;

use nomux_client::{Applied, Client, ClientKind, EventBody, EventType};
use nomux_end_to_end::Sandbox;
use serde_json::Value;

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime starts")
        .block_on(future)
}

/// Makes the directories `src/<name>` in the home of the sandbox.
fn make_directories(sandbox: &Sandbox, names: &[&str]) {
    for name in names {
        fs::create_dir_all(sandbox.home().join("src").join(name)).expect("a directory is made");
    }
}

fn tree_document(sandbox: &Sandbox) -> Value {
    sandbox
        .nomux_ok(&["group", "ls", "--tree", "--json"])
        .json()["workspace"]
        .clone()
}

#[test]
fn a_second_client_keeps_the_same_tree_with_the_events_and_a_restart_keeps_the_tree() {
    let sandbox = Sandbox::new();
    make_directories(&sandbox, &["web", "api", "notes"]);
    // The first command starts the daemon.
    assert_eq!(tree_document(&sandbox)["revision"], 0);

    let (copy, current) = block_on(async {
        let socket = sandbox.daemon_socket();
        let subscriber = Client::connect(&socket, ClientKind::Cli)
            .await
            .expect("the daemon answers");
        let mut events = subscriber
            .subscribe_events(0, &[EventType::WorkspaceChanged])
            .await
            .expect("the subscription starts");
        let mut lister = Client::connect(&socket, ClientKind::Cli)
            .await
            .expect("the daemon answers");
        let mut copy = lister.workspace().await.expect("the tree comes");

        let changes: &[&[&str]] = &[
            &["group", "new", "Client work"],
            &["group", "new", "Acme", "--in", "Client work"],
            &["project", "add", "src/web", "--group", "Client work/Acme"],
            &["project", "add", "src/api", "--group", "Acme"],
            &["project", "add", "src/notes"],
            &["group", "move", "notes", "--to", "Acme", "--before", "web"],
            &["group", "rename", "web", "acme-web"],
            &["group", "rm", "Client work"],
            &["project", "rm", "Acme/api"],
        ];
        for arguments in changes {
            let _ = sandbox.nomux_ok(arguments);
        }
        let cycle = sandbox.nomux(&["group", "move", "Acme", "--to", "Acme"]);
        assert_eq!(cycle.code, 1, "{cycle:?}");
        assert!(
            cycle
                .stderr
                .contains("a Group cannot move into itself or into a Group below it"),
            "{cycle:?}"
        );

        for _ in changes {
            let event = events
                .next()
                .await
                .expect("the event comes")
                .expect("the stream is open");
            let EventBody::WorkspaceChanged { revision, change } = event.body else {
                panic!("not a change of the tree: {event:?}");
            };
            assert_eq!(
                copy.apply(revision, &change),
                Applied::Changed,
                "{change:?}"
            );
        }
        (copy, lister.workspace().await.expect("the tree comes"))
    });

    assert_eq!(copy, current);
    assert_eq!(copy.revision, 9);
    let names: Vec<String> = copy
        .tree
        .nodes()
        .into_iter()
        .map(|node| node.name.clone())
        .collect();
    assert_eq!(names, ["Acme", "notes", "acme-web"]);

    let before = tree_document(&sandbox);
    let _ = sandbox.nomux_ok(&["daemon", "stop"]);
    assert_eq!(tree_document(&sandbox), before);
}

#[test]
fn status_tree_shows_the_sessions_below_their_groups_and_projects() {
    let sandbox = Sandbox::new();
    make_directories(&sandbox, &["web/frontend", "notes"]);
    let _ = sandbox.nomux_ok(&["group", "new", "Acme"]);
    let web = sandbox
        .nomux_ok(&["project", "add", "src/web", "--group", "Acme", "--quiet"])
        .stdout
        .trim()
        .to_owned();
    let _ = sandbox.nomux_ok(&["project", "add", "src/notes"]);
    let frontend = sandbox.home().join("src/web/frontend");
    let inside = sandbox
        .nomux_ok(&[
            "new",
            "--quiet",
            "--cwd",
            frontend.to_str().unwrap(),
            "--",
            "sleep",
            "300",
        ])
        .stdout
        .trim()
        .to_owned();
    let outside = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sleep", "300"])
        .stdout
        .trim()
        .to_owned();

    let listed = sandbox.sessions();
    let project_of = |id: &str| {
        listed
            .iter()
            .find(|session| session["id"] == id)
            .map(|session| session["project"].clone())
    };
    assert_eq!(project_of(&inside).unwrap()["path"], "Acme/web");
    assert_eq!(project_of(&outside), Some(Value::Null));

    let status = sandbox.nomux_ok(&["status", "--tree", "--json"]).json();
    let nodes = status["status"]["workspace"]
        .as_array()
        .expect("the tree is there");
    let summary: Vec<(String, Value, bool)> = nodes
        .iter()
        .map(|node| {
            (
                node["path"].as_str().unwrap().to_owned(),
                node["sessions"].clone(),
                node["aggregate"].is_object(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("Acme".to_owned(), serde_json::json!([]), true),
            ("Acme/web".to_owned(), serde_json::json!([inside]), true),
            ("notes".to_owned(), serde_json::json!([]), false),
        ]
    );
    assert_eq!(nodes[1]["id"], web);

    let text = sandbox.nomux_ok(&["status", "--tree"]).stdout;
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("  NAME"), "{text}");
    assert!(lines[1].contains(&outside), "{text}");
    assert!(lines[2].contains("Acme/"), "{text}");
    assert!(lines[3].contains("  web"), "{text}");
    assert!(lines[4].contains(&format!("    {inside}")), "{text}");
}
