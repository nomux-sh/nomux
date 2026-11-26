//! Follows the program reports of one holder: what the programs of the
//! session reported in the terminal output.
//!
//! The feed has its own connection to the holder, without the lease. It
//! keeps one `WaitProgramReports` request with the holder, thus the daemon
//! is told a change and never polls for one.

use std::io;
use std::path::Path;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::program_reports::{self, ProgramReports};
use nomux_holder_protocol::wire::{Reply, Request, SessionToken};
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::debug;

use super::backoff::Backoff;
use super::connection::{HolderConnection, open_reader};
use super::supervisor::SessionEnd;

/// What a holder says about itself when the feed connects to it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HolderFacts {
    /// The process ID of the command of the session.
    pub(crate) child_pid: u32,
    /// The token of the session. `None` for a holder that is too old to
    /// make one.
    pub(crate) token: Option<SessionToken>,
}

/// One piece of news of the feed.
pub(crate) enum ReportEvent {
    /// The feed has a connection to the holder: the first one, or a new one
    /// after the connection was lost.
    Connected(HolderFacts),
    /// The program reports of the session now.
    Reports(ProgramReports),
}

/// Gives `on_event` the program reports of the holder at `socket` at the
/// start and after each change, until the session ends. Returns how the
/// session ended.
///
/// A connection that fails is opened again after a pause that grows.
pub(crate) async fn follow_program_reports(
    socket: &Path,
    session_id: &SessionId,
    mut ended: watch::Receiver<Option<SessionEnd>>,
    mut on_event: impl FnMut(ReportEvent),
) -> SessionEnd {
    let mut backoff = Backoff::new();
    loop {
        tokio::select! {
            end = session_end(&mut ended) => return end,
            result = follow_connection(socket, session_id, &mut on_event) => match result {
                // The command has ended: the reports do not change again.
                Ok(()) => return session_end(&mut ended).await,
                Err(error) => {
                    debug!(session = %session_id, %error, "the program reports feed lost its connection");
                }
            },
        }
        tokio::select! {
            end = session_end(&mut ended) => return end,
            () = sleep(backoff.next_delay()) => {}
        }
    }
}

/// Completes when the task of the session reported its end. A task that
/// stopped without a report lost the session.
async fn session_end(ended: &mut watch::Receiver<Option<SessionEnd>>) -> SessionEnd {
    match ended.wait_for(Option::is_some).await {
        Ok(end) => end.unwrap_or(SessionEnd::Lost),
        Err(_) => SessionEnd::Lost,
    }
}

/// Follows the reports on one connection. Returns when the command of the
/// session has ended.
async fn follow_connection(
    socket: &Path,
    session_id: &SessionId,
    on_event: &mut impl FnMut(ReportEvent),
) -> io::Result<()> {
    let link = open_reader(socket, session_id).await?;
    let protocol = link.hello.protocol;
    let mut connection = link.connection;
    let token = if protocol.supports(&Request::SessionToken) {
        match ask(&mut connection, Request::SessionToken).await? {
            Reply::SessionToken(token) => Some(token),
            reply => return Err(unexpected(&reply)),
        }
    } else {
        None
    };
    on_event(ReportEvent::Connected(HolderFacts {
        child_pid: link.hello.child_pid,
        token,
    }));
    if !protocol.supports(&Request::ProgramReports) {
        // A holder that is too old to keep reports has nothing to follow.
        return std::future::pending().await;
    }
    loop {
        let reports = read_reports(&mut connection).await?;
        let known = reports.revision;
        on_event(ReportEvent::Reports(reports));
        let wait = Request::WaitProgramReports { after: known };
        match ask(&mut connection, wait).await? {
            Reply::ProgramReportsChanged { revision } if revision > known => {}
            // An answer without a change: the command has ended.
            Reply::ProgramReportsChanged { .. } => return Ok(()),
            reply => return Err(unexpected(&reply)),
        }
    }
}

async fn read_reports(connection: &mut HolderConnection) -> io::Result<ProgramReports> {
    match ask(connection, Request::ProgramReports).await? {
        Reply::ProgramReports { length, .. } => {
            let memory = connection.take_descriptor().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the reply of the holder has no descriptor",
                )
            })?;
            program_reports::read(memory, length)
        }
        reply => Err(unexpected(&reply)),
    }
}

/// Sends one request and reads its reply. The feed has one request with the
/// holder at a time, thus the next reply answers it.
async fn ask(connection: &mut HolderConnection, request: Request<'_>) -> io::Result<Reply> {
    connection.send(request).await?;
    Ok(connection.receive().await?.1)
}

fn unexpected(reply: &Reply) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the holder sent a reply that does not answer its request: {reply:?}"),
    )
}
