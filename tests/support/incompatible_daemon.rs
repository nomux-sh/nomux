//! A process that stands in the place of a daemon and refuses each client:
//! the daemon of a version that has no common protocol version with the
//! client. No real daemon has a different protocol version yet.
//!
//! As a daemon does, the process holds the lock of the runtime directory
//! (`~/.nomux/run`), writes its process ID into the lock file and listens
//! on the socket. It prints `ready` when it listens, and it ends when it
//! gets `SIGTERM`.

use std::env;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process;

use nomux_proto::wire::v1::{Envelope, Error, ErrorCode, Response, envelope, response};
use prost::Message;

fn main() {
    let home = env::var_os("HOME").expect("HOME is set");
    let runtime_directory = PathBuf::from(home).join(".nomux").join("run");
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&runtime_directory)
        .expect("the runtime directory can be made");
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(runtime_directory.join("nomuxd.lock"))
        .expect("the lock file can be opened");
    lock.lock().expect("no daemon has the lock");
    lock.set_len(0).expect("the lock file can be written");
    writeln!(lock, "{}", process::id()).expect("the lock file can be written");

    let socket = runtime_directory.join("nomuxd.sock");
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).expect("the socket can be made");
    println!("ready");

    for stream in listener.incoming().flatten() {
        refuse(stream);
    }
}

/// Reads the first request of a client and answers that the versions are
/// not compatible.
fn refuse(mut stream: UnixStream) {
    let mut bytes = Vec::new();
    let request = loop {
        let mut byte = [0u8; 1];
        if stream.read_exact(&mut byte).is_err() {
            return;
        }
        bytes.push(byte[0]);
        if let Ok(Envelope {
            kind: Some(envelope::Kind::Request(request)),
        }) = Envelope::decode_length_delimited(bytes.as_slice())
        {
            break request;
        }
    };
    let response = Envelope {
        kind: Some(envelope::Kind::Response(Response {
            id: request.id,
            result: Some(response::Result::Error(Error {
                code: ErrorCode::VersionIncompatible.into(),
                message: "this daemon is too old for the client".to_owned(),
            })),
        })),
    };
    let _ = stream.write_all(&response.encode_length_delimited_to_vec());
}
