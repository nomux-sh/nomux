//! The handlers of the Hooks service.

use nomux_core::hook_event::{MAX_HOOK_PAYLOAD, is_event_name, is_tool_name};
use nomux_proto::wire::v1::{
    Error, ErrorCode, HookAdapter, HookEvent, HookEventRequest, HookEventResponse, HookInstallPlan,
    HooksAdaptersResponse, HooksPlanRequest, HooksPlanResponse, response,
};
use prost::Message;
use tracing::debug;

use super::Context;
use super::agent::find;
use super::ext::{call_failure, failed, fitting_envelope};
use crate::hooks::{PLAN_METHOD, adapter_of, adapters};

/// `Hooks.Event`: the answer comes at once, and the adapter gets the event
/// later. The answer is the same when the tool has no adapter or when the
/// queue is full: a hook must not depend on the adapter.
pub(super) fn event(
    request: HookEventRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    check_names(&request.tool, &request.event)?;
    if request.payload.len() > MAX_HOOK_PAYLOAD {
        return Err(failed(format!(
            "the data of the hook event has {} bytes: {MAX_HOOK_PAYLOAD} at most",
            request.payload.len()
        )));
    }
    let id = find(context, &request.session_id)?;
    context
        .status
        .authenticate(&id, &request.token)
        .map_err(|refused| Error {
            code: ErrorCode::PermissionDenied.into(),
            message: format!("the hook event of session {id} was refused: {refused}"),
        })?;
    let tool = request.tool;
    let Some(extension_id) = adapter_of(&context.extensions, &tool) else {
        debug!(%tool, "no extension is the adapter of the tool: the hook event is dropped");
        return Ok(answer());
    };
    let event = HookEvent {
        session_id: id.to_string(),
        tool,
        event: request.event,
        payload: request.payload,
        payload_omitted: request.payload_omitted,
    };
    if !context.hooks.deliver(extension_id, &event) {
        debug!(tool = %event.tool, "the queue of the hook events is full: the event is dropped");
    }
    Ok(answer())
}

/// `Hooks.Plan`: the install plan of the adapter of a tool.
pub(super) async fn plan(
    request: HooksPlanRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let extension_id = adapter_of(&context.extensions, &request.tool).ok_or_else(|| Error {
        code: ErrorCode::NotFound.into(),
        message: format!("no extension is the adapter of the tool {}", request.tool),
    })?;
    let called = context
        .extensions
        .call(&extension_id, PLAN_METHOD, Vec::new());
    let payload = called
        .await
        .map_err(|error| call_failure(&extension_id, &error))?;
    let plan = HookInstallPlan::decode(payload.as_slice()).map_err(|error| {
        failed(format!(
            "the extension {extension_id} answered `{PLAN_METHOD}` with a payload that is not \
             an install plan: {error}"
        ))
    })?;
    fitting_envelope(response::Result::HooksPlan(HooksPlanResponse {
        extension_id,
        plan: Some(plan),
    }))
}

/// `Hooks.Adapters`.
pub(super) fn adapters_of_tools(context: &Context) -> response::Result {
    let adapters = adapters(&context.extensions)
        .into_iter()
        .map(|(tool, extension_id)| HookAdapter { tool, extension_id });
    response::Result::HooksAdapters(HooksAdaptersResponse {
        adapters: adapters.collect(),
    })
}

fn check_names(tool: &str, event: &str) -> Result<(), Error> {
    if !is_tool_name(tool) {
        return Err(failed(format!(
            "`{tool}` is not a tool name: a lower-case letter, then lower-case letters, digits \
             and `-`"
        )));
    }
    if !is_event_name(event) {
        return Err(failed(format!(
            "`{event}` is not an event name: 1 to 128 letters, digits, `-`, `_`, `.` and `:`"
        )));
    }
    Ok(())
}

fn answer() -> response::Result {
    response::Result::HookEvent(HookEventResponse::default())
}
