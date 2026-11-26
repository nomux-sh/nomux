//! The handlers of the Agent service.

use nomux_holder_protocol::SessionId;
use nomux_proto::wire::v1::{
    AgentExplainRequest, AgentSetStatusRequest, AgentSetStatusResponse, AgentTreeRequest, Error,
    ErrorCode, response,
};

use super::Context;
use crate::session_error::SessionError;
use crate::status::{SelfReport, TokenRefused, reported_state};

pub(super) fn tree(request: AgentTreeRequest, context: &Context) -> response::Result {
    let ids = listed_sessions(context);
    let tree = context.status.tree(&ids, request.include_children);
    response::Result::AgentTree(Box::new(tree))
}

pub(super) fn explain(
    request: &AgentExplainRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let id = find(context, &request.session_id)?;
    let explanation = context.status.explain(&id);
    Ok(response::Result::AgentExplain(Box::new(explanation)))
}

pub(super) fn set_status(
    request: &AgentSetStatusRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let id = find(context, &request.session_id)?;
    let state = reported_state(request.state).ok_or_else(|| Error {
        code: ErrorCode::Failed.into(),
        message: format!("{} is not a state of an Agent run", request.state),
    })?;
    let report = SelfReport::new(state, &request.message).map_err(|message| Error {
        code: ErrorCode::Failed.into(),
        message,
    })?;
    context
        .status
        .self_report(&id, &request.token, report)
        .map_err(|refused| refused_error(&id, refused))?;
    Ok(response::Result::AgentSetStatus(AgentSetStatusResponse {}))
}

/// The sessions that are not ended, in the sequence of their identifiers.
fn listed_sessions(context: &Context) -> Vec<SessionId> {
    context
        .sessions
        .list(false)
        .into_iter()
        .map(|listed| listed.record.id)
        .collect()
}

/// The session `id`, if it is not ended.
pub(super) fn find(context: &Context, id: &str) -> Result<SessionId, SessionError> {
    listed_sessions(context)
        .into_iter()
        .find(|listed| listed.as_str() == id)
        .ok_or_else(|| {
            SessionError::NotFound(format!(
                "no session that is not ended has the identifier {id}"
            ))
        })
}

fn refused_error(id: &SessionId, refused: TokenRefused) -> Error {
    Error {
        code: ErrorCode::PermissionDenied.into(),
        message: format!("the status of session {id} did not change: {refused}"),
    }
}
