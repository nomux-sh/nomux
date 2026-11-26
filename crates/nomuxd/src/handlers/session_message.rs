//! The form of a session and of its parts in the control protocol.

use std::ffi::OsStr;

use nomux_core::configuration::RestorePolicy;
use nomux_proto::wire::v1::{
    RestorePolicy as WireRestorePolicy, Session, SessionExit, SessionState as WireSessionState,
    session_exit,
};

use crate::holder::SessionEnd;
use crate::session_registry::{EndedSession, SessionState};
use crate::sessions::ListedSession;

impl From<ListedSession> for Session {
    fn from(listed: ListedSession) -> Self {
        let (state, pid, exit) = match listed.record.state {
            SessionState::Running { pid } => (WireSessionState::Running, pid, None),
            SessionState::Unreachable => (WireSessionState::Unreachable, 0, None),
            SessionState::Dormant => (WireSessionState::Dormant, 0, None),
            SessionState::Ended(ended) => (WireSessionState::Ended, 0, Some(ended.into())),
            SessionState::Damaged => (WireSessionState::Damaged, 0, None),
        };
        let mut session = Self {
            id: listed.record.id.to_string(),
            state: state.into(),
            pid,
            restore_policy: wire_restore_policy(listed.restore_policy).into(),
            exit,
            damage: listed.record.damage.unwrap_or_default(),
            ..Self::default()
        };
        // The metadata has no environment variable, thus the description
        // has none.
        if let Some(metadata) = listed.metadata {
            session.command = std::iter::once(&metadata.command)
                .chain(&metadata.arguments)
                .map(|word| text(word))
                .collect();
            session.working_directory = text(metadata.working_directory.as_os_str());
            session.rows = metadata.size.rows.into();
            session.columns = metadata.size.columns.into();
            session.started_at = metadata.started;
            session.epoch = metadata.epoch;
        }
        if let Some(resolved) = listed.resolved_working_directory {
            session.resolved_working_directory = text(resolved.as_os_str());
        }
        session
    }
}

impl From<EndedSession> for SessionExit {
    fn from(ended: EndedSession) -> Self {
        let status = match ended.end {
            SessionEnd::Exited { code } => Some(session_exit::Status::ExitCode(code)),
            SessionEnd::Signaled { signal } => Some(session_exit::Status::Signal(signal)),
            SessionEnd::Lost => None,
        };
        Self {
            ended_at: ended.ended_at,
            status,
        }
    }
}

fn wire_restore_policy(policy: RestorePolicy) -> WireRestorePolicy {
    match policy {
        RestorePolicy::Auto => WireRestorePolicy::Auto,
        RestorePolicy::OnFocus => WireRestorePolicy::OnFocus,
        RestorePolicy::Manual => WireRestorePolicy::Manual,
    }
}

/// The restore policy of a request. `None` if the request has none.
pub(super) fn restore_policy_of(policy: WireRestorePolicy) -> Option<RestorePolicy> {
    match policy {
        WireRestorePolicy::Auto => Some(RestorePolicy::Auto),
        WireRestorePolicy::OnFocus => Some(RestorePolicy::OnFocus),
        WireRestorePolicy::Manual => Some(RestorePolicy::Manual),
        WireRestorePolicy::Unspecified => None,
    }
}

/// A command or a path can have bytes that are not UTF-8. A message has
/// U+FFFD in their place.
fn text(word: &OsStr) -> String {
    word.to_string_lossy().into_owned()
}
