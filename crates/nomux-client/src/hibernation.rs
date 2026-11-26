//! The hibernation of an agent session, as a client sees it: the last
//! hibernation of a session, and the requests that hibernate a session and
//! pin it.

use nomux_proto::wire::v1::{
    self as wire, SessionHibernateRequest, SessionSetPinnedRequest, request, response,
};

use crate::client::unexpected_result;
use crate::session_memory::measure_of;
use crate::{Client, ClientError, MemoryMeasure, Session};

/// Why the daemon hibernated a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HibernationReason {
    /// More agent sessions were live than `agents.max_live`.
    Count,
    /// The session used more memory than `sessions.memory_limit`.
    SessionMemory,
    /// The sessions used more memory than `sessions.total_memory_limit`.
    TotalMemory,
    /// The operating system reported memory pressure.
    MemoryPressure,
    /// A client asked for it.
    Request,
    /// A reason from a later protocol version.
    Unknown,
}

/// The last hibernation of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hibernation {
    /// When the daemon stopped the command: seconds after the Unix epoch.
    pub hibernated_at: u64,
    /// Why the daemon stopped the command.
    pub reason: HibernationReason,
    /// The memory that the hibernation gave back, in bytes, and its
    /// measure. `None` if the daemon could not measure it.
    pub freed: Option<(u64, MemoryMeasure)>,
    /// The command that a wake starts.
    pub resume_command: String,
    /// When a wake started the resume command: seconds after the Unix
    /// epoch. `None` while the session is hibernated.
    pub woken_at: Option<u64>,
    /// Why the last wake failed.
    pub wake_error: Option<String>,
}

impl From<wire::SessionHibernation> for Hibernation {
    fn from(hibernation: wire::SessionHibernation) -> Self {
        let freed = (hibernation.freed_bytes != 0)
            .then(|| (hibernation.freed_bytes, measure_of(hibernation.measure)));
        Self {
            hibernated_at: hibernation.hibernated_at,
            reason: reason_of(hibernation.reason),
            freed,
            resume_command: hibernation.resume_command,
            woken_at: (hibernation.woken_at != 0).then_some(hibernation.woken_at),
            wake_error: (!hibernation.wake_error.is_empty()).then_some(hibernation.wake_error),
        }
    }
}

pub(crate) fn reason_of(reason: i32) -> HibernationReason {
    match wire::HibernationReason::try_from(reason) {
        Ok(wire::HibernationReason::Count) => HibernationReason::Count,
        Ok(wire::HibernationReason::SessionMemory) => HibernationReason::SessionMemory,
        Ok(wire::HibernationReason::TotalMemory) => HibernationReason::TotalMemory,
        Ok(wire::HibernationReason::MemoryPressure) => HibernationReason::MemoryPressure,
        Ok(wire::HibernationReason::Request) => HibernationReason::Request,
        Ok(wire::HibernationReason::Unspecified) | Err(_) => HibernationReason::Unknown,
    }
}

impl Client {
    /// Hibernates the agent session `id`: the daemon writes a checkpoint of
    /// its screen, ends its command and keeps it until a wake. The answer
    /// comes after the command has ended.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier,
    /// and [`ClientError::Failed`] with the reason if the session cannot
    /// hibernate: for example, its adapter gave no resume command, or its
    /// agent works or waits for the user.
    pub async fn hibernate_session(&mut self, id: &str) -> Result<Session, ClientError> {
        let request = SessionHibernateRequest { id: id.to_owned() };
        match self.call(request::Body::SessionHibernate(request)).await? {
            response::Result::SessionHibernate(hibernated) => hibernated
                .session
                .map(Session::from)
                .ok_or_else(|| ClientError::invalid_reply("the response has no session")),
            _ => Err(unexpected_result()),
        }
    }

    /// Excludes the session `id` from automatic hibernation with `pinned`,
    /// or takes the exclusion back.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier.
    pub async fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<(), ClientError> {
        let request = SessionSetPinnedRequest {
            id: id.to_owned(),
            pinned,
        };
        match self.call(request::Body::SessionSetPinned(request)).await? {
            response::Result::SessionSetPinned(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }
}
