//! The requests of a client for the hooks of the agent tools.

use nomux_proto::wire::v1::{
    HookEventRequest, HooksAdaptersRequest, HooksPlanRequest, request, response,
};

use crate::client::unexpected_result;
use crate::{Client, ClientError, HookAdapter, HookEvent, HookInstallPlan};

impl Client {
    /// Sends one event of a hook. The result is what the hook writes to its
    /// standard output: an answer to the tool, empty in this version.
    ///
    /// The daemon answers at once and gives the event to the adapter of the
    /// tool later. A tool without an adapter is not an error.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::PermissionDenied`] if the token is not the
    /// token of the session, [`ClientError::NotFound`] if no session has the
    /// identifier, and [`ClientError::Failed`] if a name is not valid or the
    /// payload is too large.
    pub async fn send_hook_event(&mut self, event: HookEvent) -> Result<Vec<u8>, ClientError> {
        let request = HookEventRequest {
            session_id: event.session_id,
            token: event.token,
            tool: event.tool,
            event: event.event,
            payload: event.payload,
            payload_omitted: event.payload_omitted,
        };
        match self.call(request::Body::HookEvent(request)).await? {
            response::Result::HookEvent(answer) => Ok(answer.output),
            _ => Err(unexpected_result()),
        }
    }

    /// The change that the hooks of `tool` need in its settings file, as
    /// the adapter of the tool describes it. The first request starts the
    /// adapter.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if the tool has no adapter, and
    /// [`ClientError::Failed`] if the adapter gave no valid plan.
    pub async fn hook_install_plan(&mut self, tool: &str) -> Result<HookInstallPlan, ClientError> {
        let request = HooksPlanRequest {
            tool: tool.to_owned(),
        };
        match self.call(request::Body::HooksPlan(request)).await? {
            response::Result::HooksPlan(plan) => Ok(HookInstallPlan::new(
                plan.extension_id,
                plan.plan.unwrap_or_default(),
            )),
            _ => Err(unexpected_result()),
        }
    }

    /// Each tool that has an adapter, in the sequence of the tool names.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn hook_adapters(&mut self) -> Result<Vec<HookAdapter>, ClientError> {
        let request = request::Body::HooksAdapters(HooksAdaptersRequest {});
        match self.call(request).await? {
            response::Result::HooksAdapters(list) => {
                Ok(list.adapters.into_iter().map(HookAdapter::from).collect())
            }
            _ => Err(unexpected_result()),
        }
    }
}
