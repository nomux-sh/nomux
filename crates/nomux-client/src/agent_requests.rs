//! The requests of a client for the status of the sessions of a host.

use nomux_proto::wire::v1::{
    self as wire, AgentExplainRequest, AgentSetStatusRequest, AgentTreeRequest, request, response,
};

use crate::client::unexpected_result;
use crate::{AgentExplanation, AgentRun, AgentTree, Client, ClientError, ReportedState};

impl Client {
    /// The status of each session of the host that is not ended. With
    /// `include_children`, each Agent run has its Child work.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn agent_tree(&mut self, include_children: bool) -> Result<AgentTree, ClientError> {
        let request = request::Body::AgentTree(AgentTreeRequest { include_children });
        match self.call(request).await? {
            response::Result::AgentTree(tree) => Ok((*tree).into()),
            _ => Err(unexpected_result()),
        }
    }

    /// The status of the session `id` and what each source says about it.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session that is not ended
    /// has the identifier.
    pub async fn explain_agent(&mut self, id: &str) -> Result<AgentExplanation, ClientError> {
        let request = AgentExplainRequest {
            session_id: id.to_owned(),
        };
        let response::Result::AgentExplain(explanation) =
            self.call(request::Body::AgentExplain(request)).await?
        else {
            return Err(unexpected_result());
        };
        let explanation = *explanation;
        let run = explanation
            .run
            .ok_or_else(|| ClientError::invalid_reply("the response has no Agent run"))?;
        Ok(AgentExplanation {
            run: AgentRun::from(run),
            sources: explanation.sources.into_iter().map(Into::into).collect(),
        })
    }

    /// Reports the status of the session `id` from a program in it. `token`
    /// is the value of `NOMUX_TOKEN` in the session: it proves that the
    /// request comes from that session. [`ReportedState::Idle`] takes the
    /// report back.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::PermissionDenied`] if the token is not the
    /// token of the session, [`ClientError::NotFound`] if no session has the
    /// identifier, and [`ClientError::Failed`] if the message is not one
    /// short line of text.
    pub async fn set_agent_status(
        &mut self,
        id: &str,
        token: &str,
        state: ReportedState,
        message: &str,
    ) -> Result<(), ClientError> {
        let request = AgentSetStatusRequest {
            session_id: id.to_owned(),
            token: token.to_owned(),
            state: wire::AgentState::from(state).into(),
            message: message.to_owned(),
        };
        match self.call(request::Body::AgentSetStatus(request)).await? {
            response::Result::AgentSetStatus(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }
}
