//! `hook <tool> <event>`: sends one event of a hook to the daemon of the
//! session and prints the answer for the tool.
//!
//! A hook runs in the path of the tool, thus it must not block or break the
//! tool. Without a session, without a daemon, after the deadline or after a
//! refusal, the command ends at once with no output and the exit status 0.
//! It starts no daemon, no shell and no other process.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use nomux_client::{Client, ClientError, HookEvent};

use super::report::HooksReport;
use super::standard_input;
use crate::commands::KIND;
use crate::session_environment::{DAEMON_SOCKET, SESSION_ID, TOKEN, variable};

/// The longest time to read the standard input of the hook.
const INPUT_TIME: Duration = Duration::from_millis(40);
/// The longest time of the whole command after its start: the read of the
/// input, the connection and the answer of the daemon.
const TOTAL_TIME: Duration = Duration::from_millis(80);

pub(super) async fn send(tool: String, event: String) -> HooksReport {
    let started = Instant::now();
    let output = match (variable(SESSION_ID), variable(DAEMON_SOCKET)) {
        (Some(session_id), Some(socket)) => {
            let input = standard_input::read(started + INPUT_TIME);
            let event = HookEvent {
                session_id,
                token: variable(TOKEN).unwrap_or_default(),
                tool,
                event,
                payload: input.payload,
                payload_omitted: input.omitted,
            };
            let deadline = tokio::time::Instant::from_std(started + TOTAL_TIME);
            let sent = tokio::time::timeout_at(deadline, deliver(PathBuf::from(socket), event));
            match sent.await {
                Ok(Ok(output)) => output,
                Ok(Err(error)) => {
                    report_refusal(&error);
                    Vec::new()
                }
                Err(_) => Vec::new(),
            }
        }
        // The tool does not run in a nomux session.
        _ => Vec::new(),
    };
    HooksReport::Answered { output }
}

async fn deliver(socket: PathBuf, event: HookEvent) -> Result<Vec<u8>, ClientError> {
    let mut client = Client::connect(&socket, KIND).await?;
    client.send_hook_event(event).await
}

/// A refusal goes to the standard error: the person who installed the hook
/// can see it. A daemon that does not run is not a refusal.
fn report_refusal(error: &ClientError) {
    if matches!(
        error,
        ClientError::PermissionDenied(_) | ClientError::NotFound(_) | ClientError::Failed(_)
    ) {
        eprintln!("nomux hook: {error}");
    }
}
