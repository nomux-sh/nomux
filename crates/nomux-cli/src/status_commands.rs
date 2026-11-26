//! The status commands: `status`, `status set`, `agent explain` and
//! `events`. `docs/status.md` has the model of the status.

mod arguments;
mod events;
mod report;
mod self_report;

use nomux_client::{ClientError, EventType};

pub(crate) use arguments::{AgentCommand, EventTypeArgument, StatusCommand};
pub(crate) use report::StatusReport;

use crate::commands::KIND;
use crate::output::Format;
use crate::report::Report;
use crate::selected_host;

/// `status`: the status of each session of the host.
pub(crate) async fn tree(
    host_name: Option<&str>,
    with_children: bool,
    format: Format,
) -> Result<Report, ClientError> {
    let host = selected_host::connect(host_name, format).await?;
    let mut client = host.client(KIND).await?;
    // An ended session can keep its last Agent run.
    let sessions = client.list_sessions(true).await?;
    let tree = client.agent_tree(with_children).await?;
    Ok(StatusReport::Tree {
        tree,
        sessions,
        with_children,
    }
    .into())
}

/// `status set`: the status of the session of this process. It uses the
/// daemon of the session, thus it has no `--host`.
pub(crate) async fn set(
    command: StatusCommand,
    host_name: Option<&str>,
) -> Result<Report, ClientError> {
    if host_name.is_some() {
        return Err(ClientError::Failed(
            "the --host option cannot be used with `status set`: the session is on the host of \
             this process"
                .to_owned(),
        ));
    }
    let StatusCommand::Set { state, message } = command;
    let state = state.into();
    let id = self_report::set(state, message.as_deref().unwrap_or_default()).await?;
    Ok(StatusReport::Set { id, state }.into())
}

/// `agent explain`.
pub(crate) async fn agent(
    command: AgentCommand,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let AgentCommand::Explain { id } = command;
    let host = selected_host::connect(host_name, format).await?;
    let explanation = host.client(KIND).await?.explain_agent(&id).await?;
    Ok(StatusReport::Explained(explanation).into())
}

/// `events --follow`.
pub(crate) async fn follow_events(
    filter: &[EventTypeArgument],
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let host = selected_host::connect(host_name, format).await?;
    let types: Vec<EventType> = filter.iter().map(|kind| EventType::from(*kind)).collect();
    events::follow(&host, &types, format).await?;
    Ok(StatusReport::EventsFollowed.into())
}
