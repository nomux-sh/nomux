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
    AttachEnd, AttachOptions, Client, ClientError, Host, TerminalEvent, TerminalInput,
    TerminalOutput,
};
use tokio::io::{AsyncWriteExt, Stdout};
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::sync::Notify;

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

/// Attaches to the session `id` on `host` and returns how the attach
/// ended. `host_name` is the name of a remote host, for the messages.
pub(crate) async fn run(
    host: &Host,
    host_name: Option<&str>,
    id: &str,
) -> Result<AttachEnd, ClientError> {
    require_terminal()?;
    // The attach uses one connection for the terminal. The window size goes
    // on a second connection.
    let control = host.client(KIND).await?;
    let options = AttachOptions {
        size: local_terminal::window_size(),
        resume: None,
    };
    let (mut output, mut input) = host.attach(KIND, id, options).await?;
    let signals = Signals::listen().map_err(|error| terminal_error(&error))?;
    let raw_mode = RawMode::enter().map_err(|error| terminal_error(&error))?;
    let reconnected = Notify::new();
    let mut resize = Resize { host, control, id };

    // The two directions run at the same time: a terminal that shows a
    // flood of output slowly does not keep a key waiting.
    let end = tokio::select! {
        end = show_output(&mut output, &reconnected) => end,
        end = send_input(&mut input, &mut resize, &reconnected, signals) => end,
    };
    drop(raw_mode);
    end.map_err(|error| with_attach_hint(error, host_name, id))
}

/// Writes the screen and the output of the session to the terminal until
/// the attach ends. While the attach has no connection, the title of the
/// terminal says so: the screen of the session is not changed.
async fn show_output(
    output: &mut TerminalOutput,
    reconnected: &Notify,
) -> Result<AttachEnd, ClientError> {
    let mut terminal = tokio::io::stdout();
    let mut title_is_changed = false;
    let end = loop {
        let event = match output.next().await {
            Ok(event) => event,
            Err(error) => break Err(error),
        };
        match event {
            TerminalEvent::Snapshot { bytes, starts } => {
                if starts {
                    write(&mut terminal, local_terminal::BEFORE_SNAPSHOT).await?;
                }
                write(&mut terminal, bytes).await?;
            }
            TerminalEvent::Output(bytes) => write(&mut terminal, bytes).await?,
            TerminalEvent::ConnectionLost => {
                write(&mut terminal, local_terminal::SHOW_CONNECTION_LOST).await?;
                title_is_changed = true;
            }
            TerminalEvent::Reconnected => {
                write(&mut terminal, local_terminal::HIDE_CONNECTION_LOST).await?;
                title_is_changed = false;
                reconnected.notify_one();
            }
            TerminalEvent::End(end) => break Ok(end),
        }
        terminal
            .flush()
            .await
            .map_err(|error| terminal_error(&error))?;
    };
    if title_is_changed {
        write(&mut terminal, local_terminal::HIDE_CONNECTION_LOST).await?;
        terminal
            .flush()
            .await
            .map_err(|error| terminal_error(&error))?;
    }
    end
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
    resize: &mut Resize<'_>,
    reconnected: &Notify,
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
            _ = signals.window_change.recv() => resize.send_window_size().await?,
            // The window can have a new size since the connection ended.
            () = reconnected.notified() => resize.connect_again().await?,
            () = received(&mut signals.end) => {
                return Err(ClientError::Failed("a signal ended the attach".to_owned()));
            }
        }
    }
}

/// The connection that gives the session the window size of the terminal.
struct Resize<'a> {
    host: &'a Host,
    control: Client,
    id: &'a str,
}

impl Resize<'_> {
    /// Gives the session the window size of the terminal.
    async fn send_window_size(&mut self) -> Result<(), ClientError> {
        let Some(size) = local_terminal::window_size() else {
            return Ok(());
        };
        match self.control.resize_session(self.id, size).await {
            // The session ended, or the connection ended. The output side
            // reports the first and connects again after the second.
            Err(ClientError::NotFound(_) | ClientError::Connection(_)) => Ok(()),
            result => result,
        }
    }

    /// Opens a new connection after the attach connected again, and gives
    /// the session the window size.
    async fn connect_again(&mut self) -> Result<(), ClientError> {
        match self.host.running_client(KIND).await {
            Ok(Some(control)) => {
                self.control = control;
                self.send_window_size().await
            }
            // The output side reports a connection or a daemon that is gone.
            Ok(None) | Err(ClientError::Connection(_)) => Ok(()),
            Err(error) => Err(error),
        }
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

/// A lost connection or a daemon that stopped ends the attach and not the
/// session. The message says how to continue.
fn with_attach_hint(error: ClientError, host_name: Option<&str>, id: &str) -> ClientError {
    let host_option = host_name.map_or_else(String::new, |name| format!("--host {name} "));
    let hint =
        format!("The session continues: run `nomux {host_option}attach {id}` to attach again");
    match error {
        ClientError::Connection(error) => {
            ClientError::Connection(io::Error::new(error.kind(), format!("{error}. {hint}")))
        }
        ClientError::DaemonNotRunning => ClientError::Failed(format!("the daemon stopped. {hint}")),
        error => error,
    }
}
