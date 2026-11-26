//! The status task of one session with a holder.

use std::path::PathBuf;
use std::sync::Arc;

use nomux_holder_protocol::SessionId;
use tokio::sync::watch;

use super::StatusRegistry;
use crate::holder::{HolderFacts, ReportEvent, SessionEnd, follow_program_reports};
use crate::session_registry::{SessionRegistry, SessionState};

/// Gives `status` what the holder at `socket` reports for the session
/// `id`, until the session ends. The task waits on the holder and never
/// polls. A session that is dormant after its end keeps what stays of its
/// status. A session that ended or was removed loses its status.
pub(crate) async fn watch(
    id: SessionId,
    socket: PathBuf,
    ended: watch::Receiver<Option<SessionEnd>>,
    status: Arc<StatusRegistry>,
    sessions: Arc<SessionRegistry>,
) {
    let mut holder: Option<HolderFacts> = None;
    follow_program_reports(&socket, &id, ended, |event| match event {
        ReportEvent::Connected(facts) => {
            holder = Some(facts);
            status.connected(&id, facts);
        }
        ReportEvent::Reports(reports) => status.program_reports(&id, &reports),
    })
    .await;
    // The task of the session put the session into its new state before it
    // reported the end.
    let state = sessions.find(&id).map(|(record, _)| record.state);
    match (state, holder) {
        (None | Some(SessionState::Ended(_)), _) => status.remove(&id),
        (Some(SessionState::Dormant | SessionState::Damaged), Some(facts)) => {
            status.lost_process(&id, facts);
        }
        // A restore gave the session a new holder already, or the session
        // never had a connection to a holder.
        _ => {}
    }
}
