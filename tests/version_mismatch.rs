//! The CLI and a daemon that have no common protocol version: exit code 5.
//!
//! No real daemon has a different protocol version yet. Thus the daemon of
//! these tests is a socket that answers the handshake as such a daemon does.

use std::fs::DirBuilder;
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread::{self, JoinHandle};

use nomux_end_to_end::Sandbox;
use nomux_proto::wire::v1::{
    Envelope, Error, ErrorCode, Response, Welcome, envelope, request, response,
};
use prost::Message;

const VERSION_MISMATCH: i32 = 5;

/// Listens on the daemon socket of `sandbox` and answers the `Hello` of one
/// client with `result`.
fn daemon_that_answers(sandbox: &Sandbox, result: response::Result) -> JoinHandle<()> {
    let runtime_directory = sandbox.runtime_directory();
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&runtime_directory)
        .unwrap();
    let listener = UnixListener::bind(runtime_directory.join("nomuxd.sock")).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = read_envelope(&mut stream);
        let Some(envelope::Kind::Request(request)) = hello.kind else {
            panic!("not a request: {hello:?}");
        };
        assert!(matches!(request.body, Some(request::Body::Hello(_))));
        let response = Envelope {
            kind: Some(envelope::Kind::Response(Response {
                id: request.id,
                result: Some(result),
            })),
        };
        stream
            .write_all(&response.encode_length_delimited_to_vec())
            .unwrap();
        // The CLI closes the connection when it has the answer.
        let _ = stream.read(&mut [0u8; 1]);
    })
}

fn read_envelope(stream: &mut UnixStream) -> Envelope {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        if let Ok(envelope) = Envelope::decode_length_delimited(bytes.as_slice()) {
            return envelope;
        }
    }
}

#[test]
fn a_daemon_that_refuses_the_client_version_gives_exit_code_5() {
    let sandbox = Sandbox::new();
    let daemon = daemon_that_answers(
        &sandbox,
        response::Result::Error(Error {
            code: ErrorCode::VersionIncompatible.into(),
            message: "the client is too old for this daemon".to_owned(),
        }),
    );

    let run = sandbox.nomux(&["ls"]);
    daemon.join().unwrap();

    assert_eq!(run.code, VERSION_MISMATCH, "{run:?}");
    assert_eq!(run.stdout, "");
    assert_eq!(run.stderr, "nomux: the client is too old for this daemon\n");
}

#[test]
fn a_daemon_with_a_newer_oldest_version_gives_exit_code_5_as_json() {
    let sandbox = Sandbox::new();
    let daemon = daemon_that_answers(
        &sandbox,
        response::Result::Welcome(Welcome {
            version: 9,
            oldest: 8,
            capabilities: Vec::new(),
            host_id: String::new(),
        }),
    );

    let run = sandbox.nomux(&["ls", "--json"]);
    daemon.join().unwrap();

    assert_eq!(run.code, VERSION_MISMATCH, "{run:?}");
    assert_eq!(run.stdout, "");
    let error: serde_json::Value = serde_json::from_str(&run.stderr).unwrap();
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["error"]["code"], VERSION_MISMATCH);
    assert_eq!(error["error"]["kind"], "version_mismatch");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("8 to 9"),
        "{run:?}"
    );
}
