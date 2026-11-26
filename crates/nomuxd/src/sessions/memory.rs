//! The memory of the processes of the sessions that have a holder.

use nomux_holder_protocol::discovery::{PID_FILE, holder_directory};
use tokio::task::spawn_blocking;

use super::Sessions;
use crate::session_error::SessionError;
use crate::session_memory::{self, SessionMemory};

impl Sessions {
    /// Measures the memory of each session that has a holder, in the
    /// sequence of the identifiers. The work is on a thread that is not the
    /// thread of the daemon: a host with many processes must not delay a
    /// session.
    pub(crate) async fn memory(&self) -> Result<Vec<SessionMemory>, SessionError> {
        let holders: Vec<_> = self
            .registry
            .records()
            .into_iter()
            .filter(|record| record.state.is_live())
            .map(|record| {
                let pid_file = holder_directory(&self.runtime_directory, &record.id).join(PID_FILE);
                (record.id, pid_file)
            })
            .collect();
        if holders.is_empty() {
            return Ok(Vec::new());
        }
        let measured = spawn_blocking(move || session_memory::measure(holders)).await;
        match measured {
            Ok(Ok(sessions)) => Ok(sessions),
            Ok(Err(error)) => Err(unreadable(&error)),
            Err(error) => Err(unreadable(&error)),
        }
    }
}

fn unreadable(error: &dyn std::fmt::Display) -> SessionError {
    SessionError::Failed(format!(
        "the memory of the sessions cannot be read: {error}"
    ))
}
