//! Remote hosts through SSH, with a real SSH server that each test starts:
//! `nomux host add`, the session commands with `--host`, an attach that
//! continues after the connection was cut, the trust rules for the host
//! key, the replacement of a daemon, and `nomux host doctor`.
//!
//! A test that cannot start `sshd` writes `SKIPPED` and the reason to the
//! standard error and passes.

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

use nomux_end_to_end::{
    Impairment, RemoteHost, TerminalClient, binary, generate_key, process_runs, public_key,
    wait_for,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const NO_PERMISSION: i32 = 4;
const NO_SESSIONS: [serde_json::Value; 0] = [];
const SIGTERM: i32 = 15;
/// The bytes that the CLI writes before the first part of a snapshot of the
/// screen: the start of `BEFORE_SNAPSHOT`.
const SNAPSHOT_START: &str = "\x1b[?1049l\x1b[!p\x1b[r";
/// What the CLI writes when an attach lost its connection, and when the
/// attach has a connection again.
const CONNECTION_LOST_TITLE: &str = "\x1b[22;0t\x1b]0;nomux: connection lost, connecting again\x07";
const TITLE_RESTORED: &str = "\x1b[23;0t";

fn new_session(remote: &RemoteHost, command: &[&str]) -> String {
    let arguments = [&["--host", "vps", "new", "--quiet", "--"], command].concat();
    remote.nomux_ok(&arguments).stdout.trim().to_owned()
}

fn attach(remote: &RemoteHost, id: &str) -> TerminalClient {
    remote.nomux_in_terminal(&["--host", "vps", "attach", id])
}

#[test]
fn host_add_installs_the_binaries_from_the_release_archive_and_the_session_commands_work() {
    let Some(mut remote) = RemoteHost::start_with(&[], Impairment::default()) else {
        return;
    };
    let releases = remote.release_fixture();
    remote
        .client
        .set_variable("NOMUX_RELEASE_DOWNLOAD_URL", releases);

    let added = remote
        .nomux_ok(&[
            "host",
            "add",
            &remote.ssh_target(),
            "--name",
            "vps",
            "--json",
        ])
        .json();
    assert_eq!(added["host"]["name"], "vps");
    assert_eq!(added["host"]["installed"], true);
    assert_eq!(added["host"]["daemon"]["version"], VERSION);
    assert_eq!(
        added["host"]["directory"],
        format!("~/.nomux/bin/{VERSION}")
    );

    // The host has the three binaries, and only the user can use them.
    let installation = remote.installation_directory();
    for name in ["nomux", "nomuxd", "nomux-holder"] {
        let installed = fs::read(installation.join(name)).unwrap();
        assert_eq!(installed, fs::read(binary(name)).unwrap(), "{name}");
        let mode = fs::metadata(installation.join(name)).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o700, "{name}");
    }
    for directory in [".nomux", ".nomux/bin", ".nomux/run"] {
        let mode = fs::metadata(remote.host.home().join(directory))
            .unwrap()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{directory}");
    }
    // The directory with the temporary name is gone.
    let entries = fs::read_dir(installation.parent().unwrap())
        .unwrap()
        .count();
    assert_eq!(entries, 1);

    let hosts = remote.nomux_ok(&["host", "ls", "--json"]).json();
    assert_eq!(hosts["hosts"][0]["name"], "vps");
    assert_eq!(hosts["hosts"][0]["ssh"], remote.ssh_target());

    // A second `host add` finds the binaries and installs nothing: the
    // file on the host is the same file, not a new copy.
    let installed_file = fs::metadata(installation.join("nomuxd")).unwrap().ino();
    let again = remote
        .nomux_ok(&[
            "host",
            "add",
            &remote.ssh_target(),
            "--name",
            "vps",
            "--json",
        ])
        .json();
    assert_eq!(again["host"]["installed"], false);
    assert_eq!(
        fs::metadata(installation.join("nomuxd")).unwrap().ino(),
        installed_file
    );

    // The commands of a session, as on the local host.
    let id = new_session(&remote, &["sh", "-c", "stty -echo; printf READY; exec cat"]);
    let sessions = remote.nomux_ok(&["--host", "vps", "ls", "--json"]).json();
    assert_eq!(sessions["sessions"][0]["id"], id.as_str());
    // The session is on the host and not on the local host.
    assert_eq!(remote.host.sessions().len(), 1);
    assert_eq!(remote.client.sessions(), NO_SESSIONS);

    let mut client = attach(&remote, &id);
    client.wait_for_text("READY");
    client.type_keys(b"typed on the client\r");
    client.wait_for_text("typed on the client");
    client.type_keys(b"\x1cd");
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());

    let killed = remote.nomux_ok(&["--host", "vps", "kill", &id]);
    assert_eq!(killed.stdout, format!("session {id} ended\n"));
    assert_eq!(remote.host.sessions(), NO_SESSIONS);

    assert_eq!(
        remote.nomux_ok(&["host", "rm", "vps"]).stdout,
        "host vps removed\n"
    );
    assert_eq!(remote.nomux(&["--host", "vps", "ls"]).code, 2);
}

#[test]
fn an_attach_continues_after_the_connection_was_cut_with_each_byte_one_time() {
    let Some(remote) = RemoteHost::start() else {
        return;
    };
    remote.add_as_vps();
    // The command writes the numbers from 1, one on each line, without an
    // end.
    let id = new_session(
        &remote,
        &[
            "sh",
            "-c",
            "i=0; while :; do i=$((i+1)); echo n$i.; sleep 0.01; done",
        ],
    );
    let mut client = attach(&remote, &id);
    // The client has the screen and some output after it.
    wait_for("output before the cut", || {
        client.read_pending_output();
        (numbers(&client.text_output()).len() > 20).then_some(())
    });

    remote.cut_connections();
    let numbers_at_cut = numbers(&client.text_output()).len();
    // The attach connects again, and the output continues.
    wait_for("output after the cut", || {
        client.read_pending_output();
        (numbers(&client.text_output()).len() > numbers_at_cut + 100).then_some(())
    });
    client.type_keys(b"\x1cd");
    assert_eq!(client.wait_for_exit(), 0, "{}", client.text_output());

    let output = client.text_output();
    // One snapshot: the attach after the cut got only the bytes that the
    // client did not have.
    assert_eq!(output.matches(SNAPSHOT_START).count(), 1, "{output:?}");
    // The CLI showed the lost connection in the title of the window, in
    // the middle of the output. Without the two title sequences, the
    // output is the output of the session.
    assert!(output.contains(CONNECTION_LOST_TITLE), "{output:?}");
    let output = output
        .replace(CONNECTION_LOST_TITLE, "")
        .replace(TITLE_RESTORED, "");
    // After the snapshot, each number follows the number before it: no
    // line is absent and no line comes two times.
    let after_snapshot = &output[output.rfind(SNAPSHOT_START).unwrap()..];
    let numbers = numbers(after_snapshot);
    let first_complete = &numbers[1..];
    for pair in first_complete.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "{pair:?} in {numbers:?}");
    }
    assert!(first_complete.len() > 100);
}

/// The numbers of the complete lines `n<number>.` of `output`.
fn numbers(output: &str) -> Vec<u64> {
    output
        .split(['\r', '\n'])
        .filter_map(|line| line.strip_prefix('n')?.strip_suffix('.')?.parse().ok())
        .collect()
}

#[test]
fn a_host_key_that_is_not_known_is_refused_with_the_fingerprint_and_the_ssh_command() {
    let Some(remote) = RemoteHost::start() else {
        return;
    };
    fs::remove_file(remote.known_hosts_file()).unwrap();

    let run = remote.nomux(&["host", "add", &remote.ssh_target(), "--name", "vps"]);

    assert_eq!(run.code, 1, "{run:?}");
    assert_eq!(run.stdout, "");
    let expected_start = format!(
        "nomux: the host key of {} is not known. The host sent the ssh-ed25519 key with the \
         fingerprint SHA256:",
        remote.known_hosts_name()
    );
    assert!(run.stderr.starts_with(&expected_start), "{run:?}");
    let command = format!(
        "run `ssh -o HostKeyAlgorithms=ssh-ed25519 '{}' exit` and answer `yes`",
        remote.ssh_target()
    );
    assert!(run.stderr.contains(&command), "{run:?}");
    // The client trusted nothing and saved nothing.
    assert!(!remote.known_hosts_file().exists());
    assert_eq!(remote.nomux_ok(&["host", "ls"]).stdout, "no hosts\n");
    assert!(!remote.host.home().join(".nomux/run").exists());
}

#[test]
fn a_host_key_that_changed_is_refused_with_each_rule_of_the_configuration() {
    let Some(remote) = RemoteHost::start() else {
        return;
    };
    let other_key = remote.client.home().join("other_host_key");
    generate_key(&other_key);
    remote.write_known_hosts(&public_key(&other_key.with_extension("pub")));
    fs::write(
        remote.client.home().join(".ssh/config"),
        "Host *\n  StrictHostKeyChecking no\n",
    )
    .unwrap();

    let run = remote.nomux(&["host", "add", &remote.ssh_target(), "--name", "vps"]);

    assert_eq!(run.code, 1, "{run:?}");
    let expected = format!(
        "nomux: THE HOST KEY OF {} CHANGED. {} line 1 has a different ssh-ed25519 key.",
        remote.known_hosts_name(),
        remote.known_hosts_file().display()
    );
    assert!(run.stderr.starts_with(&expected), "{run:?}");
    assert!(run.stderr.contains("ssh-keygen -R"), "{run:?}");
    assert_eq!(remote.nomux_ok(&["host", "ls"]).stdout, "no hosts\n");
}

#[test]
fn a_daemon_without_a_common_protocol_version_is_replaced_and_the_session_continues() {
    let Some(remote) = RemoteHost::start() else {
        return;
    };
    remote.add_as_vps();
    let id = new_session(&remote, &["sleep", "300"]);
    let holder = remote.host.holder_pid(&id);
    let _ = remote.host.nomux_ok(&["daemon", "stop"]);

    // A daemon of a version that refuses the client takes the place.
    let mut old_daemon = Command::new(env!("CARGO_BIN_EXE_nomux-test-incompatible-daemon"))
        .env("HOME", remote.host.home())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(old_daemon.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");

    let sessions = remote.nomux_ok(&["--host", "vps", "ls", "--json"]).json();

    // The daemon of the client stopped the old daemon and found the
    // session, and the holder is the same process.
    assert_eq!(old_daemon.wait().unwrap().signal(), Some(SIGTERM));
    assert_eq!(sessions["sessions"][0]["id"], id.as_str());
    assert_eq!(sessions["sessions"][0]["state"], "running");
    assert!(process_runs(holder));
    assert_eq!(remote.host.holder_pid(&id), holder);
    let status = remote
        .nomux_ok(&["--host", "vps", "daemon", "status", "--json"])
        .json();
    assert_eq!(status["daemon"]["version"], VERSION);
}

#[test]
fn host_doctor_passes_for_a_good_host_and_names_a_wrong_permission() {
    let Some(remote) = RemoteHost::start() else {
        return;
    };
    remote.add_as_vps();

    let good = remote.nomux_ok(&["host", "doctor", "vps"]);
    eprintln!("{}", good.stdout);
    let lines: Vec<&str> = good.stdout.lines().collect();
    assert_eq!(lines[0], "vps");
    let names: Vec<&str> = lines[1..]
        .iter()
        .map(|line| {
            let mut words = line.split_whitespace();
            assert_eq!(words.next(), Some("ok"), "{line}");
            words.next().unwrap()
        })
        .collect();
    assert_eq!(
        names,
        [
            "connection",
            "platform",
            "binaries",
            "permissions",
            "daemon",
            "versions"
        ]
    );

    // Other users can open the runtime directory now.
    remote.set_mode_on_host(".nomux/run", 0o755);
    let bad = remote.nomux(&["host", "doctor", "--json"]);
    eprintln!("{}", remote.nomux(&["host", "doctor"]).stdout);
    assert_eq!(bad.code, 1, "{bad:?}");
    let document = bad.json();
    assert_eq!(document["doctor"][0]["host"], "vps");
    assert_eq!(document["doctor"][0]["ok"], false);
    let checks = document["doctor"][0]["checks"].as_array().unwrap();
    let failed: Vec<_> = checks.iter().filter(|check| check["ok"] == false).collect();
    assert_eq!(failed.len(), 1, "{document}");
    assert_eq!(failed[0]["name"], "permissions");
    let detail = failed[0]["detail"].as_str().unwrap();
    assert!(
        detail.contains("~/.nomux/run has the mode drwxr-xr-x"),
        "{detail}"
    );
}

#[test]
fn the_keys_of_ssh_agent_authenticate_and_a_user_without_a_key_has_no_permission() {
    let Some(mut remote) = RemoteHost::start() else {
        return;
    };
    remote.add_as_vps();
    let key = remote.client.home().join(".ssh/id_ed25519");
    let socket = remote.client.home().join("agent.sock");
    let mut agent = Command::new("ssh-agent")
        .arg("-D")
        .arg("-a")
        .arg(&socket)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_for("the agent listens", || socket.exists().then_some(()));
    let added = Command::new("ssh-add")
        .arg(&key)
        .env("SSH_AUTH_SOCK", &socket)
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(added.success());
    // Only the agent has the key now.
    fs::remove_file(&key).unwrap();

    let without_agent = remote.nomux(&["--host", "vps", "ls"]);
    assert_eq!(without_agent.code, NO_PERMISSION, "{without_agent:?}");
    assert!(
        without_agent.stderr.contains("accepted no key"),
        "{without_agent:?}"
    );

    remote.client.set_variable("SSH_AUTH_SOCK", &socket);
    let with_agent = remote.nomux(&["--host", "vps", "ls"]);
    agent.kill().unwrap();
    agent.wait().unwrap();
    assert_eq!(with_agent.code, 0, "{with_agent:?}");
    assert_eq!(with_agent.stdout, "no sessions\n");
}

#[test]
fn a_host_behind_a_jump_host_is_reached_with_the_proxy_jump_of_the_configuration() {
    let Some(remote) = RemoteHost::start() else {
        return;
    };
    // The server is its own jump host: the client connects to the relay,
    // and from there to the port of the server.
    let direct = remote.direct_ssh_target();
    let port = direct.rsplit_once(':').unwrap().1;
    fs::write(
        remote.client.home().join(".ssh/config"),
        format!(
            "Host inner\n  HostName 127.0.0.1\n  Port {port}\n  ProxyJump {}\n",
            remote.ssh_target()
        ),
    )
    .unwrap();

    let _ = remote.nomux_ok(&["host", "add", "inner"]);
    let id = remote
        .nomux_ok(&["--host", "inner", "new", "--quiet", "--", "sleep", "300"])
        .stdout
        .trim()
        .to_owned();

    assert_eq!(remote.host.sessions()[0]["id"], id.as_str());
    // The connection goes through the relay: without it, the host is gone.
    remote.cut_connections();
    assert_eq!(
        remote
            .nomux_ok(&["--host", "inner", "ls", "--quiet"])
            .stdout
            .trim(),
        id
    );
}

#[test]
fn a_server_that_refuses_connections_to_unix_sockets_gives_an_error_that_names_the_options() {
    let Some(remote) =
        RemoteHost::start_with(&["AllowStreamLocalForwarding no"], Impairment::default())
    else {
        return;
    };
    remote.install_binaries();

    let run = remote.nomux(&["host", "add", &remote.ssh_target(), "--name", "vps"]);

    assert_eq!(run.code, 1, "{run:?}");
    assert!(run.stderr.contains("AllowStreamLocalForwarding"), "{run:?}");
    assert_eq!(remote.nomux_ok(&["host", "ls"]).stdout, "no hosts\n");
}
