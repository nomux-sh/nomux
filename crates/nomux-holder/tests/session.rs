//! Runs the real `nomux-holder` binary and talks to it as the daemon does:
//! discovery files, control socket and ring buffer.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_holder_protocol::discovery::{HolderState, PID_FILE, ProcessIdentity, scan};
use nomux_holder_protocol::ring::RingReader;
use nomux_holder_protocol::wire::{
    ChildSignal, ChildStatus, ErrorCode, FrameBuffer, Hello, ProtocolVersion, Reply, Request,
    WindowSize, receive_with_descriptor,
};
use tempfile::TempDir;

const HOLDER: &str = env!("CARGO_BIN_EXE_nomux-holder");
const TIMEOUT: Duration = Duration::from_secs(20);

/// A runtime directory with a short path: the path of a Unix socket has
/// about 100 bytes at most.
fn runtime_directory() -> TempDir {
    tempfile::Builder::new()
        .prefix("nomux")
        .tempdir_in("/tmp")
        .expect("the test helper works")
}

fn wait_for<T>(what: &str, mut condition: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = condition() {
            return value;
        }
        assert!(Instant::now() < deadline, "timeout: {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn holder_command(runtime: &Path, session: &str, options: &[&str], command: &[&str]) -> Command {
    let mut holder = Command::new(HOLDER);
    holder
        .args(["--session", session, "--runtime-directory"])
        .arg(runtime)
        .args(options)
        .arg("--")
        .args(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    holder
}

/// A holder that this test started. The holder is stopped when the test
/// ends before the session does.
struct Holder {
    process: Child,
}

impl Holder {
    fn start(runtime: &Path, session: &str, options: &[&str], command: &[&str]) -> Self {
        let process = holder_command(runtime, session, options, command)
            .spawn()
            .expect("the test helper works");
        Self { process }
    }

    fn wait_for_exit(&mut self) -> ExitStatus {
        wait_for("the holder exits", || {
            self.process.try_wait().expect("the test helper works")
        })
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// The daemon side of one connection to a holder.
struct Client {
    stream: UnixStream,
    frames: FrameBuffer,
    hello: Hello,
    ring: RingReader,
    output: Vec<u8>,
    next_request_id: u32,
}

impl Client {
    /// Finds the one live holder of `runtime` through its discovery files
    /// and connects to it.
    fn connect(runtime: &Path) -> Self {
        let holder = wait_for("a live holder", || {
            let mut holders = scan(runtime).expect("the test helper works");
            holders.retain(|holder| holder.state == HolderState::Live);
            holders.pop()
        });
        let stream = UnixStream::connect(holder.socket_path()).expect("the test helper works");
        stream
            .set_read_timeout(Some(TIMEOUT))
            .expect("the test helper works");

        let mut frames = FrameBuffer::new();
        let (count, descriptor) =
            receive_with_descriptor(&stream, frames.unfilled()).expect("the test helper works");
        frames.advance(count);
        let hello = Hello::decode(
            frames
                .next_frame()
                .expect("the test helper works")
                .expect("the test helper works"),
        )
        .expect("the test helper works");
        let ring = RingReader::map(descriptor.expect("the hello has the ring buffer"))
            .expect("the test helper works");
        Self {
            stream,
            frames,
            hello,
            ring,
            output: Vec::new(),
            next_request_id: 1,
        }
    }

    fn request(&mut self, request: Request<'_>) -> Reply {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let mut encoded = Vec::new();
        request.encode(request_id, &mut encoded);
        self.stream
            .write_all(&encoded)
            .expect("the test helper works");
        loop {
            if let Some(frame) = self.frames.next_frame().expect("the test helper works") {
                assert_eq!(frame.request_id, request_id);
                return Reply::decode(frame).expect("the test helper works");
            }
            let (count, _) = receive_with_descriptor(&self.stream, self.frames.unfilled())
                .expect("the test helper works");
            assert_ne!(count, 0, "the holder closed the connection");
            self.frames.advance(count);
        }
    }

    fn type_line(&mut self, line: &str) {
        let reply = self.request(Request::Input(format!("{line}\n").as_bytes()));
        assert_eq!(reply, Reply::Done);
    }

    fn read_ring(&mut self) {
        let mut buffer = [0u8; 4096];
        loop {
            match self.ring.read(&mut buffer).expect("the test helper works") {
                0 => return,
                count => self.output.extend_from_slice(&buffer[..count]),
            }
        }
    }

    /// Reads the ring buffer until the output contains `text`. Between the
    /// reads, the holder wakes the client: the client does not poll.
    fn read_until(&mut self, text: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            self.read_ring();
            let output = String::from_utf8_lossy(&self.output);
            if output.contains(text) {
                return;
            }
            assert!(Instant::now() < deadline, "no {text:?} in {output:?}");
            self.wait_output();
        }
    }

    fn wait_output(&mut self) {
        let after = self.ring.position();
        let reply = self.request(Request::WaitOutput { after });
        assert!(matches!(reply, Reply::Output { .. }), "{reply:?}");
    }

    fn wait_for_exit_status(&mut self) -> ChildStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.request(Request::Status) {
                Reply::Status(ChildStatus::Running) => {}
                Reply::Status(status) => return status,
                reply => panic!("not a status: {reply:?}"),
            }
            assert!(Instant::now() < deadline, "the child did not end");
            self.read_ring();
            self.wait_output();
        }
    }
}

fn input(bytes: &[u8]) -> Request<'_> {
    Request::Input(bytes)
}

#[test]
fn a_daemon_controls_a_shell_through_the_holder() {
    let runtime = runtime_directory();
    let mut holder = Holder::start(runtime.path(), "main", &[], &["/bin/sh"]);
    let larger = WindowSize {
        rows: 40,
        columns: 100,
    };

    let mut client = Client::connect(runtime.path());
    assert_eq!(client.hello.protocol, ProtocolVersion::CURRENT);
    assert_eq!(client.hello.session_id.as_str(), "main");
    assert_eq!(
        client.hello.size,
        WindowSize {
            rows: 24,
            columns: 80
        }
    );
    assert!(
        ProcessIdentity::of(client.hello.child_pid)
            .unwrap()
            .is_some()
    );

    // A connection must take the lease before it writes.
    assert_eq!(
        client.request(input(b"echo no\n")),
        Reply::Error(ErrorCode::LeaseRequired)
    );
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);

    // The shell computes the marker, thus the echo of the input is not it.
    client.type_line("echo marker-$((6*7))");
    client.read_until("marker-42");

    assert_eq!(client.request(Request::Resize(larger)), Reply::Done);
    client.type_line("stty size");
    client.read_until("40 100");

    // A second connection can take the lease. The first one loses it.
    let mut second = Client::connect(runtime.path());
    assert_eq!(second.hello.size, larger);
    assert_eq!(
        second.request(input(b"echo no\n")),
        Reply::Error(ErrorCode::LeaseRequired)
    );
    assert_eq!(second.request(Request::AcquireLease), Reply::Done);
    assert_eq!(
        client.request(input(b"echo no\n")),
        Reply::Error(ErrorCode::LeaseRequired)
    );

    // The session continues with no connection, and a new connection reads
    // new output.
    drop(client);
    drop(second);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.type_line("echo again-$((1+1))");
    client.read_until("again-2");
    assert_eq!(
        client.request(Request::Status),
        Reply::Status(ChildStatus::Running)
    );

    client.type_line("exit 7");
    assert_eq!(
        client.wait_for_exit_status(),
        ChildStatus::Exited { code: 7 }
    );

    // The daemon has the exit status: the holder ends and removes its files.
    assert!(holder.wait_for_exit().success());
    assert!(!runtime.path().join("main").exists());
    assert_eq!(scan(runtime.path()).unwrap(), []);
}

/// Stops the holders that a failed test left in a runtime directory.
struct StopHolders(PathBuf);

impl Drop for StopHolders {
    fn drop(&mut self) {
        for holder in scan(&self.0).unwrap_or_default() {
            let pid_file = std::fs::read_to_string(holder.directory.join(PID_FILE));
            let identity = pid_file.ok().and_then(|text| text.parse().ok());
            if let Some(ProcessIdentity { pid, .. }) = identity {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
        }
    }
}

#[test]
fn the_session_continues_after_the_starter_exits() {
    let runtime = runtime_directory();
    let _stop = StopHolders(runtime.path().to_owned());
    let starter = format!(
        "'{HOLDER}' --session orphan --runtime-directory '{}' -- /bin/sh >/dev/null 2>&1 &",
        runtime.path().display()
    );
    let status = Command::new("/bin/sh").args(["-c", &starter]).status();
    assert!(status.unwrap().success());

    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.type_line("echo alive-$((2+3))");
    client.read_until("alive-5");

    assert_eq!(
        client.request(Request::Signal(ChildSignal::Kill)),
        Reply::Done
    );
    assert_eq!(
        client.wait_for_exit_status(),
        ChildStatus::Signaled { signal: 9 }
    );
    let directory = runtime.path().join("orphan");
    wait_for("the holder removes its directory", || {
        (!directory.exists()).then_some(())
    });
}

#[test]
fn the_holder_ends_after_the_exit_timeout_when_no_daemon_reads_the_status() {
    let runtime = runtime_directory();
    let options = ["--exit-timeout", "1"];
    let command = ["/bin/sh", "-c", "exit 3"];
    let mut holder = Holder::start(runtime.path(), "brief", &options, &command);

    assert!(holder.wait_for_exit().success());
    assert!(!runtime.path().join("brief").exists());
}

#[test]
fn a_command_that_cannot_start_leaves_no_discovery_directory() {
    let runtime = runtime_directory();
    let mut holder = holder_command(runtime.path(), "none", &[], &["/nonexistent/command"]);

    let output = holder.stderr(Stdio::piped()).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot start the command"),
        "{output:?}"
    );
    assert_eq!(scan(runtime.path()).unwrap(), []);
}
