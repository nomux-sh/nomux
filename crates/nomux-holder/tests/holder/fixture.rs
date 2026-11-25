//! Starts the real `nomux-holder` binary and talks to it as the daemon does:
//! discovery files, control socket, ring buffer, redraw memory, program
//! reports memory and journal memory.

use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nomux_holder_protocol::discovery::{HolderState, scan};
use nomux_holder_protocol::journal;
use nomux_holder_protocol::program_reports::{self, ProgramReports};
use nomux_holder_protocol::redraw;
use nomux_holder_protocol::ring::RingReader;
use nomux_holder_protocol::wire::{
    ChildStatus, Frame, FrameBuffer, Hello, Reply, Request, receive_with_descriptor,
};
use tempfile::TempDir;

pub(crate) const HOLDER: &str = env!("CARGO_BIN_EXE_nomux-holder");
const TIMEOUT: Duration = Duration::from_secs(20);

/// A runtime directory with a short path: the path of a Unix socket has
/// about 100 bytes at most.
pub(crate) fn runtime_directory() -> TempDir {
    tempfile::Builder::new()
        .prefix("nomux")
        .tempdir_in("/tmp")
        .expect("the test helper works")
}

/// The state directory of the holders of a test. It is in the directory of
/// the test, thus a test writes nothing to the state directory of the user.
/// Its name is not a session identifier, thus it is no holder directory.
pub(crate) fn state_directory(runtime: &Path) -> PathBuf {
    runtime.join(".state")
}

pub(crate) fn wait_for<T>(what: &str, mut condition: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(value) = condition() {
            return value;
        }
        assert!(Instant::now() < deadline, "timeout: {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn holder_command(
    runtime: &Path,
    session: &str,
    options: &[&str],
    command: &[&str],
) -> Command {
    let mut holder = Command::new(HOLDER);
    holder
        .args(["--session", session, "--runtime-directory"])
        .arg(runtime)
        .arg("--state-directory")
        .arg(state_directory(runtime))
        .args(options)
        .arg("--")
        .args(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    holder
}

/// A holder that this test started. The holder is stopped when the test
/// ends before the session does.
pub(crate) struct Holder {
    process: Child,
}

impl Holder {
    pub(crate) fn start(runtime: &Path, session: &str, options: &[&str], command: &[&str]) -> Self {
        let process = holder_command(runtime, session, options, command)
            .spawn()
            .expect("the test helper works");
        Self { process }
    }

    /// Stops the holder with `SIGKILL`: it cannot write or remove a file.
    pub(crate) fn kill(&mut self) {
        self.process.kill().expect("the test helper works");
        self.process.wait().expect("the test helper works");
    }

    pub(crate) fn wait_for_exit(&mut self) -> ExitStatus {
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
pub(crate) struct Client {
    stream: UnixStream,
    frames: FrameBuffer,
    pub(crate) hello: Hello,
    pub(crate) ring: RingReader,
    /// The bytes that this connection read from the ring buffer.
    pub(crate) output: Vec<u8>,
    /// The descriptor that came with the last frames, if the test did not
    /// take it yet.
    descriptor: Option<OwnedFd>,
    next_request_id: u32,
}

impl Client {
    /// Finds the one live holder of `runtime` through its discovery files
    /// and connects to it.
    pub(crate) fn connect(runtime: &Path) -> Self {
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
            descriptor: None,
            next_request_id: 1,
        }
    }

    pub(crate) fn request(&mut self, request: Request<'_>) -> Reply {
        let request_id = self.send(request);
        self.receive(request_id)
    }

    /// Sends a request and does not wait for its reply. Returns the request
    /// identifier for `receive`.
    pub(crate) fn send(&mut self, request: Request<'_>) -> u32 {
        let mut encoded = Vec::new();
        request.encode(self.next_request_id, &mut encoded);
        self.send_encoded(&encoded)
    }

    /// Sends a frame that `Request` cannot make, as a later daemon can do.
    pub(crate) fn request_frame(&mut self, kind: u16, payload: &[u8]) -> Reply {
        let mut encoded = Vec::new();
        let frame = Frame {
            kind,
            request_id: self.next_request_id,
            payload,
        };
        frame.encode(&mut encoded);
        let request_id = self.send_encoded(&encoded);
        self.receive(request_id)
    }

    fn send_encoded(&mut self, encoded: &[u8]) -> u32 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.stream
            .write_all(encoded)
            .expect("the test helper works");
        request_id
    }

    /// Waits for the next reply, which must be the reply to `request_id`.
    pub(crate) fn receive(&mut self, request_id: u32) -> Reply {
        loop {
            if let Some(frame) = self.frames.next_frame().expect("the test helper works") {
                assert_eq!(frame.request_id, request_id);
                return Reply::decode(frame).expect("the test helper works");
            }
            let received = receive_with_descriptor(&self.stream, self.frames.unfilled());
            let (count, descriptor) = received.unwrap_or_else(|error| {
                let output = String::from_utf8_lossy(&self.output);
                panic!("no reply to request {request_id}: {error}, output: {output:?}")
            });
            assert_ne!(count, 0, "the holder closed the connection");
            self.frames.advance(count);
            if descriptor.is_some() {
                self.descriptor = descriptor;
            }
        }
    }

    /// Asks for the redraw sequence of the current screen. Returns the
    /// position that the screen corresponds to, and the sequence.
    pub(crate) fn redraw(&mut self) -> (u64, Vec<u8>) {
        let reply = self.request(Request::Redraw);
        let Reply::Redraw { position, length } = reply else {
            panic!("not a redraw: {reply:?}");
        };
        let memory = self
            .descriptor
            .take()
            .expect("the redraw reply has a descriptor");
        let sequence = redraw::read(memory, length).expect("the test helper works");
        (position, sequence)
    }

    /// Asks for the program reports of the holder.
    pub(crate) fn program_reports(&mut self) -> ProgramReports {
        let reply = self.request(Request::ProgramReports);
        let Reply::ProgramReports { revision, length } = reply else {
            panic!("not the program reports: {reply:?}");
        };
        let memory = self
            .descriptor
            .take()
            .expect("the program reports reply has a descriptor");
        let reports = program_reports::read(memory, length).expect("the test helper works");
        assert_eq!(reports.revision, revision);
        reports
    }

    /// Asks for output bytes of the journal from `from` on. Returns the
    /// position of the first byte and the bytes.
    pub(crate) fn journal(&mut self, from: u64, max_length: u32) -> (u64, Vec<u8>) {
        let reply = self.request(Request::JournalRead { from, max_length });
        let Reply::Journal { position, length } = reply else {
            panic!("not a journal range: {reply:?}");
        };
        if length == 0 {
            return (position, Vec::new());
        }
        let memory = self
            .descriptor
            .take()
            .expect("a journal reply with bytes has a descriptor");
        let bytes = journal::read_range(memory, length).expect("the test helper works");
        (position, bytes)
    }

    pub(crate) fn type_line(&mut self, line: &str) {
        let reply = self.request(Request::Input(format!("{line}\n").as_bytes()));
        assert_eq!(reply, Reply::Done);
    }

    pub(crate) fn read_ring(&mut self) {
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
    pub(crate) fn read_until(&mut self, text: &str) {
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

    /// Reads the ring buffer until the output ends with `end`.
    pub(crate) fn read_until_end(&mut self, end: &[u8]) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            self.read_ring();
            if self.output.ends_with(end) {
                return;
            }
            assert!(Instant::now() < deadline, "no {end:x?} at the end");
            self.wait_output();
        }
    }

    pub(crate) fn wait_output(&mut self) {
        let after = self.ring.position();
        let reply = self.request(Request::WaitOutput { after });
        assert!(matches!(reply, Reply::Output { .. }), "{reply:?}");
    }

    /// Waits until the holder wrote `position` bytes of output or more.
    /// Does not read the ring buffer. Returns the write position.
    pub(crate) fn wait_for_position(&mut self, position: u64) -> u64 {
        let deadline = Instant::now() + TIMEOUT;
        let mut after = 0;
        loop {
            let reply = self.request(Request::WaitOutput { after });
            let Reply::Output { write_position } = reply else {
                panic!("not an output position: {reply:?}");
            };
            if write_position >= position {
                return write_position;
            }
            assert!(Instant::now() < deadline, "no position {position}");
            after = write_position;
        }
    }

    pub(crate) fn wait_for_exit_status(&mut self) -> ChildStatus {
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
