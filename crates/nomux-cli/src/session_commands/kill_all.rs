//! `kill --all`: ends each session whose command runs. A command that ends
//! many sessions never runs without a sign of the user: on a terminal it
//! asks one question with the number of sessions, and without a terminal it
//! needs `--yes`.

use std::io::{self, BufRead, IsTerminal, Write};

use nomux_client::{Client, ClientError, Session, SessionState};

use super::arguments::KillAllOptions;
use crate::exit_code;
use crate::report::Report;

pub(super) async fn kill_all(
    mut client: Client,
    arguments: &KillAllOptions,
) -> Result<Report, ClientError> {
    let sessions: Vec<Session> = client
        .list_sessions(false)
        .await?
        .into_iter()
        .filter(|session| ends(session, arguments.include_dormant))
        .collect();
    if arguments.dry_run || sessions.is_empty() {
        return Ok(Report::SessionsKilled {
            sessions,
            dry_run: arguments.dry_run,
            failure: None,
        });
    }
    if !arguments.yes {
        confirm(sessions.len())?;
    }
    let mut killed = Vec::new();
    let mut failure = None;
    for session in sessions {
        match client.kill_session(&session.id).await {
            // Not found: the session ended between the list and the request.
            Ok(()) | Err(ClientError::NotFound(_)) => killed.push(session),
            Err(error) => {
                eprintln!("nomux: session {}: {error}", session.label());
                let code = exit_code::of_error(&error);
                failure = Some(failure.map_or(code, |earlier: u8| earlier.max(code)));
            }
        }
    }
    Ok(Report::SessionsKilled {
        sessions: killed,
        dry_run: false,
        failure,
    })
}

/// A dormant or a damaged session loses its output with a kill. After a
/// restart of the host each session is dormant, thus only
/// `--include-dormant` takes these sessions.
fn ends(session: &Session, include_dormant: bool) -> bool {
    match session.state {
        SessionState::Running | SessionState::Unreachable => true,
        SessionState::Dormant | SessionState::Damaged => include_dormant,
        SessionState::Ended | SessionState::Unknown => false,
    }
}

/// Asks the user on the terminal. Fails if the user does not answer yes,
/// or if no terminal can show the question.
fn confirm(count: usize) -> Result<(), ClientError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(ClientError::Failed(format!(
            "kill --all ends {count} sessions: without a terminal, give --yes"
        )));
    }
    let sessions = if count == 1 { "session" } else { "sessions" };
    eprint!("nomux: end {count} {sessions}? [y/N] ");
    let mut answer = String::new();
    io::stderr()
        .flush()
        .and_then(|()| io::stdin().lock().read_line(&mut answer))
        .map_err(|error| ClientError::Failed(format!("the terminal cannot be used: {error}")))?;
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(ClientError::Failed("no session ended".to_owned()))
    }
}
