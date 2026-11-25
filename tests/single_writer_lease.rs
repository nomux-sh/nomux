//! Two daemons cannot write to the same holder.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use nomux_end_to_end::{Sandbox, process_runs, wait_for};
use nomux_holder_protocol::discovery::SOCKET_FILE;
use nomux_holder_protocol::wire::{FrameBuffer, Hello, Reply, Request};

/// Connects to the holder as a second daemon does and takes the lease.
fn take_lease(sandbox: &Sandbox, id: &str) -> UnixStream {
    let socket = sandbox.runtime_directory().join(id).join(SOCKET_FILE);
    let mut stream = UnixStream::connect(socket).unwrap();
    let mut frames = FrameBuffer::new();
    let mut next_frame = |stream: &mut UnixStream| {
        loop {
            if let Some(frame) = frames.next_frame().unwrap() {
                return (frame.kind, frame.payload.to_vec());
            }
            let read = stream.read(frames.unfilled()).unwrap();
            assert_ne!(read, 0, "the holder closed the connection");
            frames.advance(read);
        }
    };

    let (kind, payload) = next_frame(&mut stream);
    let frame = nomux_holder_protocol::wire::Frame {
        kind,
        request_id: 0,
        payload: &payload,
    };
    Hello::decode(frame).unwrap();

    let mut request = Vec::new();
    Request::AcquireLease.encode(1, &mut request);
    stream.write_all(&request).unwrap();
    let (kind, payload) = next_frame(&mut stream);
    let frame = nomux_holder_protocol::wire::Frame {
        kind,
        request_id: 1,
        payload: &payload,
    };
    assert_eq!(Reply::decode(frame).unwrap(), Reply::Done);
    stream
}

#[test]
fn a_daemon_that_lost_the_lease_does_not_write_and_stops() {
    let sandbox = Sandbox::new();
    let created = sandbox.nomux_ok(&["new", "--json", "--cmd", "sh"]).json();
    let id = created["session"]["id"].as_str().unwrap().to_owned();
    let pid = u32::try_from(created["session"]["pid"].as_u64().unwrap()).unwrap();
    let daemon = sandbox.daemon_pid();

    let second_daemon = take_lease(&sandbox, &id);

    let refused = sandbox.nomux(&["kill", &id]);
    assert_eq!(refused.code, 1, "{refused:?}");
    assert!(refused.stderr.contains("lease"), "{refused:?}");
    assert!(process_runs(pid), "the command must not get the signal");
    wait_for("the daemon without the lease stopped", || {
        (!process_runs(daemon)).then_some(())
    });

    // The next daemon takes the lease back, as a daemon update does.
    drop(second_daemon);
    assert_eq!(
        sandbox.nomux_ok(&["kill", &id]).stdout,
        format!("session {id} ended\n")
    );
    assert!(!process_runs(pid));
}
