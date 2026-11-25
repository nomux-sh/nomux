//! The attach of the client library with the real daemon and a real holder:
//! what a client that attaches again gets, and the requests for the
//! terminal of a session.

use nomux_client::{
    AttachOptions, Client, ClientError, ClientKind, OutputPosition, TerminalEvent, TerminalInput,
    TerminalOutput, WindowSize,
};
use nomux_end_to_end::{Sandbox, kill_process, process_runs, wait_for};

/// The command writes each byte that it reads, and the terminal changes no
/// byte. Thus the output after `READY` is exactly the input.
const ECHO_COMMAND: &str = "stty raw -echo; printf READY; exec cat";

/// More than the ring buffer of a holder, which has 262144 bytes.
const MORE_THAN_THE_RING: usize = 300_000;

async fn connect(sandbox: &Sandbox) -> Client {
    Client::connect(&sandbox.daemon_socket(), ClientKind::Cli)
        .await
        .expect("the daemon answers")
}

/// One attach. The attach ends when the value is dropped.
struct Attached {
    output: TerminalOutput,
    _input: TerminalInput,
}

async fn attach(sandbox: &Sandbox, id: &str, resume: Option<OutputPosition>) -> Attached {
    let options = AttachOptions { size: None, resume };
    let (output, input) = connect(sandbox).await.attach(id, options).await.unwrap();
    Attached {
        output,
        _input: input,
    }
}

/// What an attach gave until its bytes contained a text.
struct Received {
    /// `true` if the daemon sent a snapshot of the screen.
    snapshot: bool,
    /// The output bytes, without the bytes of a snapshot.
    output: Vec<u8>,
}

/// Reads the events until the bytes of the events contain `text`.
async fn receive_until(attached: &mut Attached, text: &[u8]) -> Received {
    let mut received = Received {
        snapshot: false,
        output: Vec::new(),
    };
    let mut all = Vec::new();
    while !all.windows(text.len()).any(|window| window == text) {
        match attached.output.next().await.expect("the attach continues") {
            TerminalEvent::Snapshot { bytes, .. } => {
                received.snapshot = true;
                all.extend_from_slice(bytes);
            }
            TerminalEvent::Output(bytes) => {
                received.output.extend_from_slice(bytes);
                all.extend_from_slice(bytes);
            }
            event => panic!("an attach of one connection gave {event:?}"),
        }
    }
    received
}

#[tokio::test]
async fn a_client_that_attaches_again_gets_only_the_output_that_it_does_not_have() {
    let sandbox = Sandbox::new();
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh", "-c", ECHO_COMMAND])
        .stdout
        .trim()
        .to_owned();
    let mut control = connect(&sandbox).await;

    // A first attach has no position: it gets the screen.
    let mut terminal = attach(&sandbox, &id, None).await;
    assert!(receive_until(&mut terminal, b"READY").await.snapshot);
    control.send_input(&id, b"one").await.unwrap();
    assert_eq!(receive_until(&mut terminal, b"one").await.output, b"one");
    let position = terminal.output.position().unwrap();
    drop(terminal);

    // The command writes while no client is attached.
    control.send_input(&id, b"two").await.unwrap();
    let mut terminal = attach(&sandbox, &id, Some(position)).await;
    let missed = receive_until(&mut terminal, b"two").await;
    assert!(!missed.snapshot);
    assert_eq!(missed.output, b"two");
    let position = terminal.output.position().unwrap();
    drop(terminal);

    // The holder counts the output, thus a new daemon continues the numbers.
    let daemon = sandbox.daemon_pid();
    kill_process(daemon);
    wait_for("the daemon is gone", || {
        (!process_runs(daemon)).then_some(())
    });
    // `ls` starts a new daemon.
    assert_eq!(sandbox.sessions().len(), 1);
    assert_ne!(sandbox.daemon_pid(), daemon);
    let mut control = connect(&sandbox).await;
    control.send_input(&id, b"three").await.unwrap();
    let mut terminal = attach(&sandbox, &id, Some(position)).await;
    let missed = receive_until(&mut terminal, b"three").await;
    assert!(!missed.snapshot);
    assert_eq!(missed.output, b"three");
    drop(terminal);

    // A position that the session did not reach is not a position of this
    // session: the client gets the screen.
    let later = OutputPosition {
        epoch: position.epoch,
        seq: position.seq + 1_000_000,
    };
    let mut terminal = attach(&sandbox, &id, Some(later)).await;
    control.send_input(&id, b"four").await.unwrap();
    assert!(receive_until(&mut terminal, b"four").await.snapshot);
    drop(terminal);

    // The daemon has only the last output of a session. A client that was
    // away for more output gets the screen.
    let mut flood = vec![b'x'; MORE_THAN_THE_RING];
    flood.extend_from_slice(b"five");
    control.send_input(&id, &flood).await.unwrap();
    let mut terminal = attach(&sandbox, &id, None).await;
    receive_until(&mut terminal, b"five").await;
    drop(terminal);
    let mut terminal = attach(&sandbox, &id, Some(position)).await;
    control.send_input(&id, b"six").await.unwrap();
    assert!(receive_until(&mut terminal, b"six").await.snapshot);
}

#[tokio::test]
async fn a_request_for_the_terminal_of_a_session_reports_what_is_not_possible() {
    let sandbox = Sandbox::new();
    let id = sandbox
        .nomux_ok(&["new", "--quiet", "--", "sh", "-c", ECHO_COMMAND])
        .stdout
        .trim()
        .to_owned();
    let mut control = connect(&sandbox).await;

    // A window with more cells than a holder permits.
    let too_large = WindowSize {
        rows: 2000,
        columns: 2000,
    };
    let refused = control.resize_session(&id, too_large).await;
    assert!(
        matches!(refused, Err(ClientError::Failed(_))),
        "{refused:?}"
    );
    let options = AttachOptions {
        size: Some(too_large),
        resume: None,
    };
    let refused = connect(&sandbox).await.attach(&id, options).await;
    assert!(
        matches!(refused, Err(ClientError::Failed(_))),
        "{refused:?}"
    );

    for unknown in ["no-session", "not a session identifier"] {
        let size = WindowSize {
            rows: 24,
            columns: 80,
        };
        let resized = control.resize_session(unknown, size).await;
        assert!(
            matches!(resized, Err(ClientError::NotFound(_))),
            "{resized:?}"
        );
        let written = control.send_input(unknown, b"x").await;
        assert!(
            matches!(written, Err(ClientError::NotFound(_))),
            "{written:?}"
        );
    }

    // The connection continues after the errors, and the session too.
    control.send_input(&id, b"still here").await.unwrap();
    let mut terminal = attach(&sandbox, &id, None).await;
    assert!(receive_until(&mut terminal, b"still here").await.snapshot);
}
