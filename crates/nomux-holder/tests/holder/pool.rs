//! A pooled holder: it runs its command before its session exists, writes
//! no file of a session until the assignment, and ends when the pipe of its
//! daemon closes while it is pooled.

use std::fs;
use std::os::unix::ffi::OsStrExt;

use nomux_holder_protocol::journal::JournalReader;
use nomux_holder_protocol::session_state::{
    JOURNAL_DIRECTORY, METADATA_FILE, SessionMetadata, session_state_directory,
};
use nomux_holder_protocol::wire::{ErrorCode, POOLED_EPOCH, Reply, Request, WindowSize};

use crate::fixture::{Client, Holder, runtime_directory, state_directory, wait_for};

const SIZE: WindowSize = WindowSize {
    rows: 30,
    columns: 100,
};

#[test]
fn a_pooled_holder_writes_the_files_of_its_session_only_at_the_assignment() {
    let runtime = runtime_directory();
    let work = runtime.path().join("work");
    fs::create_dir(&work).expect("the test helper works");
    let (_holder, _pipe) = Holder::start_pooled(
        runtime.path(),
        "pooled",
        &["/bin/sh", "-c", "read line; echo got $line; read end"],
    );
    let mut client = Client::connect_pooled(runtime.path());
    let session_files =
        session_state_directory(&state_directory(runtime.path()), &"pooled".parse().unwrap());

    assert_eq!(client.hello.epoch, POOLED_EPOCH);
    assert!(runtime.path().join("pooled.pool").is_dir());
    assert!(!runtime.path().join("pooled").exists());
    assert!(
        !session_files.exists(),
        "a pooled holder wrote session files"
    );
    let assignment = Request::AssignSession {
        size: SIZE,
        working_directory: work.as_os_str().as_bytes(),
    };
    assert_eq!(
        client.request(assignment),
        Reply::Error(ErrorCode::LeaseRequired)
    );

    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    client.type_line("before");
    client.read_until("got before");
    let position = client.ring.position();
    assert_eq!(client.request(assignment), Reply::Done);

    assert!(runtime.path().join("pooled").is_dir());
    assert!(!runtime.path().join("pooled.pool").exists());
    let metadata = fs::read_to_string(session_files.join(METADATA_FILE)).unwrap();
    let metadata = SessionMetadata::parse(&metadata).unwrap();
    assert_eq!(metadata.working_directory, work);
    assert_eq!(metadata.size, SIZE);
    assert_eq!(metadata.epoch, 1);
    // The journal starts at the assignment with the screen of that moment.
    let reader = JournalReader::open(&session_files.join(JOURNAL_DIRECTORY)).unwrap();
    let checkpoint = reader
        .checkpoint_before(1, position)
        .unwrap()
        .expect("a checkpoint at the assignment");
    assert_eq!(checkpoint.position, position);
    assert_eq!(checkpoint.size, SIZE);
    assert!(String::from_utf8_lossy(&checkpoint.redraw_sequence).contains("got before"));

    // A session has one assignment. A connection finds the holder by the
    // name of its session.
    assert_eq!(
        client.request(assignment),
        Reply::Error(ErrorCode::InvalidPayload)
    );
    let second = Client::connect(runtime.path());
    assert_eq!(second.hello.epoch, 1);
    assert_eq!(second.hello.size, SIZE);
}

#[test]
fn a_pooled_holder_ends_when_the_pipe_closes_and_an_assigned_one_continues() {
    let runtime = runtime_directory();
    let (mut pooled, pipe) = Holder::start_pooled(runtime.path(), "unused", &["/bin/sh"]);
    let _client = Client::connect_pooled(runtime.path());
    drop(pipe);
    assert!(pooled.wait_for_exit().success());
    wait_for("the pooled holder directory is gone", || {
        (!runtime.path().join("unused.pool").exists()).then_some(())
    });

    let (_assigned, pipe) = Holder::start_pooled(runtime.path(), "assigned", &["/bin/sh"]);
    let mut client = Client::connect_pooled(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    let assignment = Request::AssignSession {
        size: client.hello.size,
        working_directory: b"/",
    };
    assert_eq!(client.request(assignment), Reply::Done);
    drop(pipe);
    client.type_line("echo still-here");
    client.read_until("still-here");
}

#[test]
fn a_holder_that_started_with_its_session_refuses_an_assignment() {
    let runtime = runtime_directory();
    let _holder = Holder::start(runtime.path(), "session", &[], &["/bin/sh"]);
    let mut client = Client::connect(runtime.path());
    assert_eq!(client.request(Request::AcquireLease), Reply::Done);
    let assignment = Request::AssignSession {
        size: client.hello.size,
        working_directory: b"/",
    };
    assert_eq!(
        client.request(assignment),
        Reply::Error(ErrorCode::InvalidPayload)
    );
}
