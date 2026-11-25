//! The handlers of the Session service.

use std::path::PathBuf;

use nomux_holder_protocol::wire::WindowSize;
use nomux_proto::wire::v1::{
    Error, ErrorCode, Session, SessionAttachRequest, SessionCreateRequest, SessionCreateResponse,
    SessionDetachRequest, SessionDetachResponse, SessionInputRequest, SessionInputResponse,
    SessionKillRequest, SessionKillResponse, SessionListResponse, SessionResizeRequest,
    SessionResizeResponse, SessionState as WireSessionState, response,
};

use super::Context;
use crate::attachment::Attachment;
use crate::session_error::SessionError;
use crate::session_registry::{SessionRecord, SessionState};

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
    context.sessions.find(&request.id)?.kill().await?;
    Ok(response::Result::SessionKill(SessionKillResponse {}))
}

pub(super) async fn attach(
    request: &SessionAttachRequest,
    context: &Context,
) -> Result<Attachment, Error> {
    // Without a window size, the session keeps the size that it has.
    let size = match (request.rows, request.columns) {
        (0, 0) => None,
        (rows, columns) => Some(window_size(rows, columns)?),
    };
    Ok(Attachment::prepare(request, size, &context.sessions).await?)
}

pub(super) fn detach(
    request: &SessionDetachRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let clients = context.sessions.find(&request.id)?.clients.detach_all();
    Ok(response::Result::SessionDetach(SessionDetachResponse {
        detached: u32::try_from(clients).unwrap_or(u32::MAX),
    }))
}

pub(super) async fn input(
    request: SessionInputRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let session = context.sessions.find_reachable(&request.id)?;
    session.write_input(request.data).await?;
    Ok(response::Result::SessionInput(SessionInputResponse {}))
}

pub(super) async fn resize(
    request: &SessionResizeRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let size = window_size(request.rows, request.columns)?;
    let session = context.sessions.find_reachable(&request.id)?;
    session.resize(size).await?;
    Ok(response::Result::SessionResize(SessionResizeResponse {}))
}

/// The window size of a request, if the holder accepts it.
fn window_size(rows: u32, columns: u32) -> Result<WindowSize, Error> {
    let size = match (u16::try_from(rows), u16::try_from(columns)) {
        (Ok(rows @ 1..), Ok(columns @ 1..)) => Some(WindowSize { rows, columns }),
        _ => None,
    };
    size.filter(|size| size.cells() <= WindowSize::MAX_CELLS)
        .ok_or_else(|| Error {
            code: ErrorCode::Failed.into(),
            message: format!(
                "a window of {rows} rows and {columns} columns is not valid: each value is \
                 from 1 to 65535, and a window has {} cells at most",
                WindowSize::MAX_CELLS
            ),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_size_is_valid_only_inside_the_limits_of_a_holder() {
        let valid = |rows, columns| window_size(rows, columns).is_ok();

        assert!(valid(1, 1));
        assert!(valid(24, 80));
        assert!(valid(16, 65535));
        // 1024 times 1024 is the largest number of cells.
        assert!(valid(1024, 1024));
        assert!(!valid(1024, 1025));
        assert!(!valid(0, 80));
        assert!(!valid(24, 0));
        // A value that does not fit in the 16 bits of the holder protocol.
        assert!(!valid(1, 65536));
        assert!(!valid(65536, 1));
    }
}
