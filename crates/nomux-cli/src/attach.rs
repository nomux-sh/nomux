//! The `attach` command: the terminal of the user becomes the terminal of a
//! session until the user detaches or the session ends.
//!
//! The client library moves the bytes. This module is the terminal side:
//! raw mode, the keys, the detach sequence, the window size and the signals.

mod detach_key;
mod keyboard;
mod local_terminal;

use std::future::poll_fn;
use std::io;
use std::task::Poll;

use nomux_client::{
    AttachEnd, AttachOptions, Client, ClientError, TerminalEvent, TerminalInput, TerminalOutput,
};
use tokio::io::{AsyncWriteExt, Stdout};
use tokio::signal::unix::{Signal, SignalKind, signal};

use self::detach_key::DetachKey;
use self::local_terminal::RawMode;
use crate::commands::KIND;

/// Fails if the command cannot use the terminal of the user.
pub(crate) fn require_terminal() -> Result<(), ClientError> {
    if local_terminal::is_available() {
        Ok(())
    } else {
        Err(ClientError::Failed(
            "attach needs a terminal: the standard input is not a terminal".to_owned(),
        ))
    }
}

/// Attaches to the session `id` and returns how the attach ended.
pub(crate) async fn run(id: &str) -> Result<AttachEnd, ClientError> {
    require_terminal()?;
    // The attach uses one connection for the terminal. The window size goes
    // on a second connection.
    let mut control = Client::connect_local_or_start(KIND).await?;
    let options = AttachOptions {
        size: local_terminal::window_size(),
        resume: None,
    };
    let (mut output, mut input) = Client::connect_local(KIND)
        .await?
        .attach(id, options)
        .await?;
    let signals = Signals::listen().map_err(|error| terminal_error(&error))?;
    let raw_mode = RawMode::enter().map_err(|error| terminal_error(&error))?;

    // The two directions run at the same time: a terminal that shows a
    // flood of output slowly does not keep a key waiting.
    let end = tokio::select! {
        end = show_output(&mut output) => end,
        end = send_input(&mut input, &mut control, id, signals) => end,
    };
    drop(raw_mode);
    end.map_err(|error| with_attach_hint(error, id))
}

/// Writes the screen and the output of the session to the terminal until
/// the attach ends.
async fn show_output(output: &mut TerminalOutput) -> Result<AttachEnd, ClientError> {
    let mut terminal = tokio::io::stdout();
    loop {
        match output.next().await? {
            TerminalEvent::Snapshot { bytes, starts } => {
                if starts {
                    write(&mut terminal, local_terminal::BEFORE_SNAPSHOT).await?;
                }
                write(&mut terminal, bytes).await?;
            }
            TerminalEvent::Output(bytes) => write(&mut terminal, bytes).await?,
            TerminalEvent::End(end) => return Ok(end),
        }
        terminal
            .flush()
            .await
            .map_err(|error| terminal_error(&error))?;
    }
}

async fn write(terminal: &mut Stdout, bytes: &[u8]) -> Result<(), ClientError> {
    terminal
        .write_all(bytes)
        .await
        .map_err(|error| terminal_error(&error))
}

/// Sends the keys and the window size of the user to the session. Returns
/// when the user detaches or the terminal closes.
async fn send_input(
    input: &mut TerminalInput,
    control: &mut Client,
    id: &str,
    mut signals: Signals,
) -> Result<AttachEnd, ClientError> {
    let mut keys = keyboard::read_keys();
    let mut detach_key = DetachKey::default();
    let mut forward = Vec::new();
    loop {
        tokio::select! {
            typed = keys.recv() => {
                let Some(typed) = typed else {
                    return Ok(AttachEnd::Detached);
                };
                forward.clear();
                let detach = detach_key.filter(&typed, &mut forward);
                if !forward.is_empty() {
                    input.send(&forward).await?;
                }
                if detach {
                    return Ok(AttachEnd::Detached);
                }
            }
            _ = signals.window_change.recv() => resize(control, id).await?,
            () = received(&mut signals.end) => {
                return Err(ClientError::Failed("a signal ended the attach".to_owned()));
            }
        }
    }
}

/// Gives the session the new window size of the terminal.
async fn resize(control: &mut Client, id: &str) -> Result<(), ClientError> {
    let Some(size) = local_terminal::window_size() else {
        return Ok(());
    };
    match control.resize_session(id, size).await {
        // The session ended. The output side reports that.
        Err(ClientError::NotFound(_)) => Ok(()),
        result => result,
    }
}

/// The signals of the process during an attach.
struct Signals {
    /// The window of the terminal has a new size.
    window_change: Signal,
    /// The signals that end a process. The attach ends without them
    /// stopping the process, thus the terminal gets its mode back.
    end: [Signal; 4],
}

impl Signals {
    fn listen() -> io::Result<Self> {
        Ok(Self {
            window_change: signal(SignalKind::window_change())?,
            end: [
                signal(SignalKind::terminate())?,
                signal(SignalKind::hangup())?,
                signal(SignalKind::interrupt())?,
                signal(SignalKind::quit())?,
            ],
        })
    }
}

/// Completes when the process gets one of `signals`.
async fn received(signals: &mut [Signal]) {
    poll_fn(|context| {
        let received = signals
            .iter_mut()
            .any(|signal| signal.poll_recv(context).is_ready());
        if received {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
}

fn terminal_error(error: &io::Error) -> ClientError {
    ClientError::Failed(format!("the terminal cannot be used: {error}"))
}

/// A lost connection ends the attach and not the session. The message says
/// how to continue.
fn with_attach_hint(error: ClientError, id: &str) -> ClientError {
    match error {
        ClientError::Connection(error) => ClientError::Connection(io::Error::new(
            error.kind(),
            format!("{error}. The session continues: run `nomux attach {id}` to attach again"),
        )),
        error => error,
    }
}
