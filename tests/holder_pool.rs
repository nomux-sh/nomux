//! The holder pool of the daemon with the real binaries: a new session with
//! the shell takes a pooled holder and works as a session with a new
//! holder, a pooled bash, zsh or fish shows only its prompt in the directory
//! of the session, the pool starts only after the first request of a client
//! and empties after its idle time, and no pooled holder stays without its
//! daemon.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_client::{Client, ClientKind, NewSession};
use nomux_end_to_end::{Sandbox, binary, kill_process, process_runs, wait_for};
use nomux_holder_protocol::discovery::{
    HolderState, PID_FILE, ProcessIdentity, pooled_holder_directory, scan_pooled,
};
use nomux_holder_protocol::journal::JournalReader;
use nomux_holder_protocol::session_state::JOURNAL_DIRECTORY;

/// The pool of the default configuration.
const POOL_SIZE: usize = 2;
/// The target of `bench/README.md` for a new session from the pool.
const NEW_SESSION_TARGET: Duration = Duration::from_millis(10);
/// The pool changes the directory of this shell.
const POOLED_SHELL: &str = "/bin/bash";

/// A sandbox whose shell is [`POOLED_SHELL`].
fn sandbox_with_bash() -> Sandbox {
    let mut sandbox = Sandbox::new();
    sandbox.set_variable("SHELL", POOLED_SHELL);
    sandbox
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime starts")
        .block_on(future)
}

/// The identifiers of the live pooled holders, when the pool has `count`.
fn wait_for_pool(sandbox: &Sandbox, count: usize) -> HashSet<String> {
    wait_for("the pool has its holders", || {
        let live: HashSet<String> = scan_pooled(&sandbox.runtime_directory())
            .expect("the runtime directory can be read")
            .into_iter()
            .filter(|holder| holder.state == HolderState::Live)
            .map(|holder| holder.session_id.to_string())
            .collect();
        (live.len() == count).then_some(live)
    })
}

fn pooled_holder_pid(sandbox: &Sandbox, id: &str) -> u32 {
    let directory = pooled_holder_directory(
        &sandbox.runtime_directory(),
        &id.parse().expect("a session identifier"),
    );
    let identity: ProcessIdentity = fs::read_to_string(directory.join(PID_FILE))
        .expect("the pooled holder has a pid file")
        .parse()
        .expect("the pid file is valid");
    identity.pid
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

fn wait_for_capture(sandbox: &Sandbox, id: &str, text: &str) {
    wait_for(&format!("the output of session {id} has {text:?}"), || {
        sandbox
            .nomux_ok(&["capture", id])
            .stdout
            .contains(text)
            .then_some(())
    });
}

fn new_session_in(sandbox: &Sandbox, directory: &Path) -> String {
    let mut command = sandbox.nomux_command(&["new", "--quiet"]);
    let output = command
        .current_dir(directory)
        .stdin(Stdio::null())
        .output()
        .expect("the CLI starts");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("text")
        .trim()
        .to_owned()
}

/// A working directory with a space. The path has no symbolic link: the
/// CLI sends the current directory, and `/tmp` is a link on macOS.
fn project_directory(sandbox: &Sandbox) -> PathBuf {
    let directory = sandbox.home().join("a project");
    fs::create_dir(&directory).expect("the directory can be made");
    fs::canonicalize(directory).expect("the directory exists")
}

#[test]
fn a_session_from_the_pool_works_as_a_session_with_a_new_holder() {
    let sandbox = sandbox_with_bash();
    let project = project_directory(&sandbox);
    let _ = sandbox.sessions();
    let pooled = wait_for_pool(&sandbox, POOL_SIZE);

    let id = new_session_in(&sandbox, &project);

    assert!(pooled.contains(&id), "session {id} is not from {pooled:?}");
    let listed = sandbox.sessions();
    assert_eq!(listed.len(), 1, "a pooled holder is a session: {listed:?}");
    assert_eq!(listed[0]["state"], "running");
    assert_eq!(listed[0]["rows"], 24);
    assert_eq!(listed[0]["columns"], 80);
    assert_eq!(listed[0]["cwd"], project.to_str().expect("text"));
    assert_eq!(listed[0]["epoch"], 1);

    // The shell is in the directory of the session and has the variables of
    // the session. The token proves the session to the daemon.
    type_line(
        &sandbox,
        &id,
        &format!(
            "echo \"[$PWD|$NOMUX_SESSION_ID|${{#NOMUX_TOKEN}}]\"; {} status set working",
            binary("nomux").display()
        ),
    );
    wait_for_capture(&sandbox, &id, &format!("[{}|{id}|32]", project.display()));
    wait_for("the self-report arrives", || {
        let tree = sandbox.nomux_ok(&["status", "--tree", "--json"]).json();
        tree["status"]["sessions"]
            .as_array()?
            .iter()
            .any(|run| run["id"] == id.as_str() && run["status"]["source"] == "self_report")
            .then_some(())
    });
    // An attach shows the screen: the prompt in the directory of the
    // session, and the output of a key that the terminal types.
    let mut terminal = sandbox.nomux_in_terminal(&["attach", &id]);
    terminal.wait_for_text(&format!("[{}|{id}|32]", project.display()));
    terminal.type_keys(b"echo at''tached\r");
    terminal.wait_for_text("attached");
    drop(terminal);

    // The pool fills again after the take.
    let refilled = wait_for_pool(&sandbox, POOL_SIZE);
    assert!(!refilled.contains(&id));

    // A restart of the host: the session is dormant, and it starts again
    // in its directory with the journal of the first epoch.
    sandbox.kill_each_process();
    let listed = sandbox.sessions();
    assert_eq!(listed[0]["id"], id.as_str());
    assert_eq!(listed[0]["state"], "dormant");
    let _ = sandbox.nomux_ok(&["start", &id]);
    type_line(&sandbox, &id, "echo \"(again in $PWD)\"");
    wait_for_capture(&sandbox, &id, &format!("(again in {})", project.display()));
}

/// The time from the request of a client that is connected to the response:
/// the session then accepts input. The median of 11 sessions, each with a
/// full pool, also in a debug build.
#[test]
fn a_session_from_the_pool_opens_in_less_than_10_ms() {
    let sandbox = sandbox_with_bash();
    let project = project_directory(&sandbox);
    let _ = sandbox.sessions();
    let mut times = Vec::new();
    for _ in 0..11 {
        let pooled = wait_for_pool(&sandbox, POOL_SIZE);
        let (id, time) = block_on(async {
            let mut client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
                .await
                .expect("the daemon answers");
            let start = Instant::now();
            let session = client
                .create_session(NewSession {
                    command: Vec::new(),
                    working_directory: Some(project.clone()),
                })
                .await
                .expect("the session starts");
            (session.id, start.elapsed())
        });
        assert!(pooled.contains(&id), "session {id} is not from {pooled:?}");
        times.push(time);
    }
    times.sort();
    assert!(times[times.len() / 2] < NEW_SESSION_TARGET, "{times:?}");
}

/// The path of the program `name` in `PATH`.
fn program_in_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

/// A start file that prints a line and sets a prompt with the full path of
/// the working directory, for each shell.
fn write_start_file(sandbox: &Sandbox, shell: &str) {
    let (path, text) = match shell {
        "bash" => (".bashrc", "PS1='[$PWD]\\$ '\necho start file\n"),
        "zsh" => (".zshrc", "PROMPT='[%/]%# '\nprint start file\n"),
        _ => (
            ".config/fish/config.fish",
            "set -g fish_greeting\nfunction fish_prompt; echo -n \"[$PWD]> \"; end\necho start file\n",
        ),
    };
    let path = sandbox.home().join(path);
    fs::create_dir_all(path.parent().expect("a directory")).expect("the directory can be made");
    fs::write(path, text).expect("the start file can be written");
}

/// The rows of the screen of the session `id` in an attach, when it shows
/// the prompt in `directory`.
fn screen_with_prompt(sandbox: &Sandbox, id: &str, directory: &Path) -> Vec<String> {
    let mut terminal = sandbox.nomux_in_terminal(&["attach", id]);
    terminal.wait_for_text(&format!("[{}]", directory.display()))
}

/// A pooled shell runs the start files of the user while it waits, and
/// changes to the directory of the session before its first prompt. Thus
/// the screen is the screen of a session with a new holder, and neither the
/// screen nor the journal has a line that the daemon typed.
#[test]
fn a_pooled_shell_shows_only_its_prompt_in_the_directory_of_the_session() {
    for shell in ["bash", "zsh", "fish"] {
        let Some(path) = program_in_path(shell) else {
            eprintln!("SKIPPED for {shell}: the program is not in PATH");
            continue;
        };
        let mut sandbox = Sandbox::new();
        sandbox.set_variable("SHELL", &path);
        // No terminal answers the queries of fish: it would wait for them.
        sandbox.set_variable("fish_features", "no-query-term");
        write_start_file(&sandbox, shell);
        // A name that a shell reads in different ways as a word.
        let project = sandbox.home().join("it's a $project");
        fs::create_dir(&project).expect("the directory can be made");
        let project = fs::canonicalize(project).expect("the directory exists");
        let _ = sandbox.sessions();
        let pooled = wait_for_pool(&sandbox, POOL_SIZE);

        let id = new_session_in(&sandbox, &project);
        assert!(
            pooled.contains(&id),
            "{shell}: session {id} is not from {pooled:?}"
        );
        let screen = screen_with_prompt(&sandbox, &id, &project);
        // A command gets a new holder.
        let path_text = path.to_str().expect("text");
        let new_holder = sandbox
            .nomux_ok(&[
                "new",
                "--quiet",
                "--cwd",
                project.to_str().expect("text"),
                "--",
                path_text,
            ])
            .stdout
            .trim()
            .to_owned();
        let expected = screen_with_prompt(&sandbox, &new_holder, &project);

        assert_eq!(screen, expected, "{shell}");
        assert!(
            screen.iter().any(|row| row == "start file"),
            "{shell}: {screen:?}"
        );
        let journal = sandbox.session_state_directory(&id).join(JOURNAL_DIRECTORY);
        let output = wait_for(&format!("{shell}: the journal has the prompt"), || {
            let range = JournalReader::open(&journal)
                .ok()?
                .read(1, 0, 65536)
                .ok()??;
            let text = String::from_utf8_lossy(&range.bytes).into_owned();
            text.contains(&format!("[{}]", project.display()))
                .then_some(text)
        });
        assert!(!output.contains(" cd "), "{shell}: {output:?}");
        // The named pipe is gone, and the shell has no variable of nomux.
        let fifo = sandbox
            .runtime_directory()
            .join("pool")
            .join(format!("{id}.directory"));
        assert!(!fifo.exists(), "{shell}: {} stays", fifo.display());
        type_line(
            &sandbox,
            &id,
            "env | grep -c -e NOMUX_DIRECTORY_FIFO -e NOMUX_ZDOTDIR -e ZDOTDIR",
        );
        wait_for(
            &format!("{shell}: the shell has no variable of nomux"),
            || {
                let output = sandbox.nomux_ok(&["capture", &id]).stdout;
                // The count is a line of its own in the raw output.
                output
                    .split(['\r', '\n'])
                    .any(|part| part == "0")
                    .then_some(())
            },
        );
    }
}

/// Starts a session with the shell through the client library. `None` is
/// the home directory.
fn new_shell_session(sandbox: &Sandbox, working_directory: Option<PathBuf>) -> String {
    block_on(async {
        let mut client = Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
            .await
            .expect("the daemon answers");
        client
            .create_session(NewSession {
                command: Vec::new(),
                working_directory,
            })
            .await
            .expect("the session starts")
            .id
    })
}

#[test]
fn without_the_start_files_of_nomux_the_pool_takes_only_sessions_in_the_home_directory() {
    let mut sandbox = sandbox_with_bash();
    sandbox.write_configuration("[pool]\nshell_start_files = false\n");
    let project = project_directory(&sandbox);
    let _ = sandbox.sessions();
    let pooled = wait_for_pool(&sandbox, POOL_SIZE);

    let in_project = new_shell_session(&sandbox, Some(project));
    let in_home = new_shell_session(&sandbox, None);

    assert!(
        !pooled.contains(&in_project),
        "{in_project} is from {pooled:?}"
    );
    assert!(
        pooled.contains(&in_home),
        "{in_home} is not from {pooled:?}"
    );
}

#[test]
fn no_pooled_holder_runs_before_the_first_request_or_after_the_idle_time() {
    let mut sandbox = Sandbox::new();
    sandbox.write_configuration("[pool]\nidle_time = \"2s\"\n");
    let daemon = Command::new(binary("nomuxd"))
        .arg("--detach")
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.home().join("config"))
        .env("SHELL", "/bin/sh")
        .status()
        .expect("the daemon starts");
    assert!(daemon.success());
    wait_for("the daemon listens", || {
        sandbox.daemon_socket().exists().then_some(())
    });
    thread::sleep(Duration::from_millis(500));
    assert!(
        scan_pooled(&sandbox.runtime_directory())
            .unwrap()
            .is_empty(),
        "a pooled holder started before a request"
    );

    let _ = sandbox.sessions();
    let pooled = wait_for_pool(&sandbox, POOL_SIZE);
    let pids: Vec<u32> = pooled
        .iter()
        .map(|id| pooled_holder_pid(&sandbox, id))
        .collect();

    // No request for the idle time: the pool empties, and its holders and
    // their shells end.
    wait_for("the pool is empty", || {
        scan_pooled(&sandbox.runtime_directory())
            .unwrap()
            .is_empty()
            .then_some(())
    });
    // A holder removes its discovery directory before its process ends.
    wait_for("the pooled holders end", || {
        pids.iter().all(|pid| !process_runs(*pid)).then_some(())
    });
    assert_eq!(sandbox.sessions().len(), 0, "a pooled holder is a session");
}

#[test]
fn the_pooled_holders_end_with_their_daemon_and_a_new_daemon_has_its_own() {
    let sandbox = Sandbox::new();
    let _ = sandbox.sessions();
    let earlier = wait_for_pool(&sandbox, POOL_SIZE);
    let pids: Vec<u32> = earlier
        .iter()
        .map(|id| pooled_holder_pid(&sandbox, id))
        .collect();

    // `SIGKILL`: the daemon does nothing at its end. The pipes close.
    kill_process(sandbox.daemon_pid());
    wait_for("the pooled holders end", || {
        pids.iter().all(|pid| !process_runs(*pid)).then_some(())
    });

    assert_eq!(sandbox.sessions().len(), 0, "a pooled holder is a session");
    let later = wait_for_pool(&sandbox, POOL_SIZE);
    assert!(earlier.is_disjoint(&later), "{earlier:?} and {later:?}");
}
