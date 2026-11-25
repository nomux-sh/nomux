//! Runs the real `nomuxd` binary and talks to it as a client does.

use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_proto::wire::v1::{
    Envelope, ErrorCode, Hello, HostInfoRequest, Request, envelope, request, response,
};
use nomux_transport::{EnvelopeStream, SOCKET_FILE};
use rustix::process::{Pid, Signal, kill_process};
use tempfile::TempDir;

const NOMUXD: &str = env!("CARGO_BIN_EXE_nomuxd");
const TIMEOUT: Duration = Duration::from_secs(20);

/// A home directory with a short path: the path of a Unix socket has about
/// 100 bytes at most.
fn home_directory() -> TempDir {
    tempfile::Builder::new()
        .prefix("nomux")
        .tempdir_in("/tmp")
        .expect("the test helper works")
}

fn nomuxd(home: &TempDir) -> Command {
    let mut command = Command::new(NOMUXD);
    command
        .env("HOME", home.path())
        .env_remove("NOMUX_LOG")
        .stdin(Stdio::null());
    command
}

fn socket_path(home: &TempDir) -> PathBuf {
    home.path().join(".nomux").join("run").join(SOCKET_FILE)
}

/// A daemon in the foreground that this test started. It is stopped when
/// the test ends.
struct Daemon {
    process: Child,
    home: TempDir,
}

impl Daemon {
    /// Starts a daemon and waits until its socket accepts a connection.
    fn start() -> Self {
        let home = home_directory();
        let process = nomuxd(&home)
            .stderr(Stdio::null())
            .spawn()
            .expect("the test helper works");
        let daemon = Self { process, home };
        let deadline = Instant::now() + TIMEOUT;
        while UnixStream::connect(daemon.socket()).is_err() {
            assert!(Instant::now() < deadline, "the daemon did not listen");
            thread::sleep(Duration::from_millis(10));
        }
        daemon
    }

    fn socket(&self) -> PathBuf {
        socket_path(&self.home)
    }

    /// Runs a second `nomuxd` for the same home directory to its end.
    fn start_second(&self) -> Output {
        nomuxd(&self.home).output().expect("the test helper works")
    }

    async fn connect(&self) -> EnvelopeStream {
        nomux_transport::connect(&self.socket())
            .await
            .expect("the test helper works")
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Sends one request and returns the result of its response.
async fn call(stream: &mut EnvelopeStream, id: u64, body: request::Body) -> response::Result {
    let request = Envelope {
        kind: Some(envelope::Kind::Request(Request {
            id,
            body: Some(body),
        })),
    };
    stream.send(&request).await.expect("the test helper works");
    let envelope = stream
        .receive()
        .await
        .expect("the test helper works")
        .expect("the test helper works");
    let Some(envelope::Kind::Response(response)) = envelope.kind else {
        panic!("not a response: {envelope:?}");
    };
    assert_eq!(response.id, id);
    response.result.expect("the test helper works")
}

fn hello(oldest: u32, version: u32) -> request::Body {
    request::Body::Hello(Hello {
        version,
        oldest,
        ..Hello::default()
    })
}

#[tokio::test]
async fn a_second_daemon_ends_with_an_error_and_the_first_one_continues() {
    let mut first = Daemon::start();

    let second = first.start_second();
    assert_eq!(second.status.code(), Some(1));
    let message = String::from_utf8(second.stderr).unwrap();
    assert!(message.contains("a daemon already runs"), "{message}");
    assert!(
        message.contains(&format!("process ID {}", first.process.id())),
        "{message}"
    );

    // The second daemon did not remove the socket of the first one.
    assert_eq!(first.process.try_wait().unwrap(), None);
    let mut stream = first.connect().await;
    let response::Result::Welcome(_) = call(&mut stream, 1, hello(1, 1)).await else {
        panic!("the first daemon does not answer the handshake");
    };
    let response::Result::HostInfo(info) =
        call(&mut stream, 2, request::Body::HostInfo(HostInfoRequest {})).await
    else {
        panic!("the first daemon does not answer a request");
    };
    assert_eq!(info.daemon_pid, first.process.id());
}

#[tokio::test]
async fn a_client_without_a_common_version_gets_error_code_5_and_no_connection() {
    let daemon = Daemon::start();

    // No protocol version is older than version 1. A client that is too new
    // uses the same rule and the same answer as a client that is too old.
    for (oldest, version, text) in [(2, 2, "2 to 2"), (0, 0, "not valid")] {
        let mut stream = daemon.connect().await;
        let response::Result::Error(error) = call(&mut stream, 1, hello(oldest, version)).await
        else {
            panic!("the daemon accepted version {oldest} to {version}");
        };
        assert_eq!(error.code, ErrorCode::VersionIncompatible as i32);
        assert!(error.message.contains(text), "{}", error.message);
        assert_eq!(stream.receive().await.unwrap(), None);
    }
}

#[tokio::test]
async fn a_request_before_the_handshake_is_refused_and_the_connection_closes() {
    let daemon = Daemon::start();
    let mut stream = daemon.connect().await;

    let result = call(&mut stream, 1, request::Body::HostInfo(HostInfoRequest {})).await;

    let response::Result::Error(error) = result else {
        panic!("the daemon answered a request without a handshake: {result:?}");
    };
    assert_eq!(error.code, ErrorCode::Failed as i32);
    assert_eq!(stream.receive().await.unwrap(), None);
}

#[test]
fn sigterm_stops_the_daemon_and_removes_the_socket() {
    let mut daemon = Daemon::start();
    let pid = Pid::from_child(&daemon.process);

    kill_process(pid, Signal::TERM).unwrap();

    let status = daemon.process.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(!daemon.socket().exists());
}

#[test]
fn a_runtime_directory_that_other_users_can_open_is_refused() {
    let home = home_directory();
    DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(home.path().join(".nomux").join("run"))
        .unwrap();
    // The mode of a new directory depends on the umask of the test process.
    let run = home.path().join(".nomux").join("run");
    fs::set_permissions(&run, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let output = nomuxd(&home).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let message = String::from_utf8(output.stderr).unwrap();
    assert!(message.contains("mode 755"), "{message}");
    assert!(!socket_path(&home).exists());
}
