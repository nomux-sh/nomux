//! `events --follow`: prints each event of the host when it comes, until
//! the daemon closes the connection or a signal stops the command.

use std::io::{self, Write};

use nomux_client::{ClientError, EventType, Host};
use tokio::signal::unix::{SignalKind, signal};

use crate::commands::KIND;
use crate::output::{self, Format};

pub(super) async fn follow(
    host: &Host,
    types: &[EventType],
    format: Format,
) -> Result<(), ClientError> {
    let mut interrupt = signal(SignalKind::interrupt()).map_err(ClientError::Connection)?;
    let mut terminate = signal(SignalKind::terminate()).map_err(ClientError::Connection)?;
    let mut events = host.client(KIND).await?.subscribe_events(0, types).await?;
    loop {
        tokio::select! {
            _ = interrupt.recv() => return Ok(()),
            _ = terminate.recv() => return Ok(()),
            event = events.next() => {
                let Some(event) = event? else {
                    return Ok(());
                };
                let line = output::event_line(&event, format);
                // A reader that closed the pipe wants no more events.
                if write_line(&line).is_err() {
                    return Ok(());
                }
            }
        }
    }
}

fn write_line(line: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(line.as_bytes())?;
    stdout.flush()
}
