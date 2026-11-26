//! `status set`: a program reports the status of the session that it runs
//! in. The holder of the session gives each process of it the variables
//! that the request needs.

use std::path::PathBuf;

use nomux_client::{Client, ClientError, ReportedState};

use crate::commands::KIND;
use crate::session_environment::{DAEMON_SOCKET, SESSION_ID, TOKEN};

/// Reports `state` and `message` for the session of this process. Returns
/// the session identifier.
pub(super) async fn set(state: ReportedState, message: &str) -> Result<String, ClientError> {
    let id = variable(SESSION_ID)?;
    let token = variable(TOKEN)?;
    let socket = PathBuf::from(variable(DAEMON_SOCKET)?);
    let mut client = Client::connect(&socket, KIND).await?;
    client.set_agent_status(&id, &token, state, message).await?;
    Ok(id)
}

/// A variable that only a process of a session has. Without it, the
/// command cannot prove which session it is in.
fn variable(name: &str) -> Result<String, ClientError> {
    crate::session_environment::variable(name).ok_or_else(|| {
        ClientError::PermissionDenied(format!(
            "{name} is not set: `nomux status set` works only inside a nomux session"
        ))
    })
}
