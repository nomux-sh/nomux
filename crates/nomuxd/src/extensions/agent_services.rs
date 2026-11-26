//! The host services of an adapter extension: `agent.report` and
//! `agent.child`. `spec/adapter.md` of the `proto` repository has their
//! messages and rules.

use nomux_ext_host::{ErrorCode, ServiceError};
use nomux_holder_protocol::SessionId;
use nomux_proto::wire::v1::{AgentReport, AgentReportResult, ChildWorkReport};
use prost::Message;

use crate::status::{AdapterReportRefused, ChildReport, RunReport, StatusRegistry};

/// The status of the Agent run of a session: an `AgentReport`.
pub(super) const AGENT_REPORT: &str = "agent.report";
/// One Child work of the Agent run of a session: a `ChildWorkReport`.
pub(super) const AGENT_CHILD: &str = "agent.child";
/// The scope of the two services.
pub(super) const AGENT_REPORT_SCOPE: &str = "agent:report";

/// Does `agent.report` with the encoded `payload`.
pub(super) fn report_run(status: &StatusRegistry, payload: &[u8]) -> Result<Vec<u8>, ServiceError> {
    let report = AgentReport::decode(payload).map_err(|error| not_readable(&error))?;
    let id = session_id(&report.session_id)?;
    let checked = RunReport::new(&report).map_err(invalid)?;
    status
        .adapter_run(&id, checked)
        .map_err(|refused| refused_error(&id, refused))?;
    Ok(AgentReportResult {}.encode_to_vec())
}

/// Does `agent.child` with the encoded `payload`.
pub(super) fn report_child(
    status: &StatusRegistry,
    payload: &[u8],
) -> Result<Vec<u8>, ServiceError> {
    let report = ChildWorkReport::decode(payload).map_err(|error| not_readable(&error))?;
    let id = session_id(&report.session_id)?;
    let checked = ChildReport::new(&report).map_err(invalid)?;
    status
        .adapter_child(&id, checked)
        .map_err(|refused| refused_error(&id, refused))?;
    Ok(AgentReportResult {}.encode_to_vec())
}

fn session_id(id: &str) -> Result<SessionId, ServiceError> {
    id.parse()
        .map_err(|_| invalid(format!("`{id}` is not a session identifier")))
}

fn refused_error(id: &SessionId, refused: AdapterReportRefused) -> ServiceError {
    let code = match refused {
        AdapterReportRefused::NoProcess => ErrorCode::NotFound,
        AdapterReportRefused::Refused(_) => ErrorCode::InvalidArgument,
    };
    ServiceError::new(
        code,
        format!("the status of session {id} did not change: {refused}"),
    )
}

fn not_readable(error: &prost::DecodeError) -> ServiceError {
    invalid(format!("the payload is not a valid message: {error}"))
}

fn invalid(message: String) -> ServiceError {
    ServiceError::new(ErrorCode::InvalidArgument, message)
}
