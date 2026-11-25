//! The handlers of the Session service.

use std::path::PathBuf;

use nomux_proto::wire::v1::{
    Error, ErrorCode, Session, SessionCreateRequest, SessionCreateResponse, SessionKillRequest,
    SessionKillResponse, SessionListResponse, SessionState as WireSessionState, response,
};

use super::Context;
use crate::session_registry::{SessionRecord, SessionState};
use crate::sessions::SessionError;

pub(super) async fn create(
    request: SessionCreateRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let working_directory =
        (!request.working_directory.is_empty()).then(|| PathBuf::from(request.working_directory));
    let session = context
        .sessions
        .create(request.command, working_directory)
        .await?;
    Ok(response::Result::SessionCreate(SessionCreateResponse {
        session: Some(session.into()),
    }))
}

pub(super) fn list(context: &Context) -> response::Result {
    let sessions = context.sessions.list();
    response::Result::SessionList(SessionListResponse {
        sessions: sessions.into_iter().map(Session::from).collect(),
    })
}

pub(super) async fn kill(
    request: &SessionKillRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    context.sessions.kill(&request.id).await?;
    Ok(response::Result::SessionKill(SessionKillResponse {}))
}

impl From<SessionRecord> for Session {
    fn from(record: SessionRecord) -> Self {
        let (state, pid) = match record.state {
            SessionState::Running { pid } => (WireSessionState::Running, pid),
            SessionState::Unreachable => (WireSessionState::Unreachable, 0),
        };
        Self {
            id: record.id.to_string(),
            state: state.into(),
            pid,
        }
    }
}

impl From<SessionError> for Error {
    fn from(error: SessionError) -> Self {
        let code = match error {
            SessionError::NotFound(_) => ErrorCode::NotFound,
            SessionError::Timeout(_) => ErrorCode::Timeout,
            SessionError::Failed(_) => ErrorCode::Failed,
        };
        Self {
            code: code.into(),
            message: error.to_string(),
        }
    }
}
