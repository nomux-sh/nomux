//! `capture --follow`: the last lines of a session, and then each byte that
//! its command writes, until the session ends or a signal stops the
//! command. It also writes the output of a session to a file:
//! `nomux capture -f api >> api.log`.
//!
//! The command reads the lines and their end position, and then attaches
//! from that position without a window size and without input. Thus it
//! gets the bytes after the lines once, and no screen.

use std::io;

use nomux_client::{AttachOptions, Client, ClientError, Host, TerminalEvent};
use tokio::io::{AsyncWriteExt, Stdout};
use tokio::signal::unix::{SignalKind, signal};

use crate::commands::KIND;
use crate::report::Report;

pub(super) async fn follow(
    host: &Host,
    mut client: Client,
    id: &str,
    lines: usize,
) -> Result<Report, ClientError> {
    let (output, end) = client.capture_tail(id, lines).await?;
    let mut standard_output = tokio::io::stdout();
    if !write(&mut standard_output, &output).await? {
        return Ok(Report::Followed);
    }
    let options = AttachOptions {
        size: None,
        resume: Some(end),
    };
    let (mut terminal, _input) = host.attach(KIND, id, options).await?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(|error| output_error(&error))?;
    let mut terminate = signal(SignalKind::terminate()).map_err(|error| output_error(&error))?;
    loop {
        let event = tokio::select! {
            event = terminal.next() => event?,
            _ = interrupt.recv() => break,
            _ = terminate.recv() => break,
        };
        match event {
            TerminalEvent::Output(bytes) => {
                if !write(&mut standard_output, bytes).await? {
                    break;
                }
            }
            TerminalEvent::End(_) => break,
            // A screen is not output, and a lost connection connects again.
            TerminalEvent::Snapshot { .. }
            | TerminalEvent::HistoryGap
            | TerminalEvent::Restored
            | TerminalEvent::ConnectionLost
            | TerminalEvent::Reconnected => {}
        }
    }
    Ok(Report::Followed)
}

/// Writes `bytes`. Returns `false` if the reader closed the output: the
/// command then ends without an error.
async fn write(standard_output: &mut Stdout, bytes: &[u8]) -> Result<bool, ClientError> {
    let written = match standard_output.write_all(bytes).await {
        Ok(()) => standard_output.flush().await,
        Err(error) => Err(error),
    };
    match written {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(output_error(&error)),
    }
}

fn output_error(error: &io::Error) -> ClientError {
    ClientError::Failed(format!("the output cannot be written: {error}"))
}
