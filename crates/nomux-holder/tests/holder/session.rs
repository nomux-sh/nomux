//! The life of a session: control requests, the lease, the exit status and
//! the discovery files.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use nomux_holder_protocol::discovery::{PID_FILE, ProcessIdentity, scan};
use nomux_holder_protocol::wire::{
    ChildSignal, ChildStatus, ErrorCode, ProtocolVersion, Reply, Request, WindowSize,
};

use crate::fixture::{Client, HOLDER, Holder, holder_command, runtime_directory, wait_for};

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
