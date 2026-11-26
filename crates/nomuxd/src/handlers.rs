//! The request handlers: each one changes a request of the protocol into a
//! call of the daemon and the result into a response of the protocol.

mod agent;
mod events;
mod ext;
mod host;
mod session;
mod session_memory;
mod session_message;

use std::sync::Arc;

use nomux_ext_host::ExtensionHost;
use nomux_proto::wire::v1::{Error, ErrorCode, request, response};

use crate::attachment::Attachment;
use crate::events::{EventLog, Subscription};
use crate::handshake::ClientFeatures;
use crate::in_flight_requests::InFlightRequests;
use crate::sessions::Sessions;
use crate::shutdown::Shutdown;
use crate::status::StatusRegistry;

/// What the handlers of all connections share.
pub(crate) struct Context {
    pub(crate) sessions: Arc<Sessions>,
    pub(crate) status: Arc<StatusRegistry>,
    pub(crate) events: Arc<EventLog>,
    pub(crate) extensions: ExtensionHost,
    pub(crate) shutdown: Shutdown,
    pub(crate) in_flight_requests: InFlightRequests,
}

/// What the connection does after a request.
pub(crate) enum Outcome {
    /// Send the response. The connection stays a control channel.
    Respond(response::Result),
    /// Send the response of the attach. Then the connection carries the
    /// terminal frames of the attach.
    Attach(Attachment),
    /// Send the response of the subscription. Then the connection carries
    /// the events of the host.
    Subscribe(Subscription),
}

/// Does one request of a connection that has `features`.
pub(crate) async fn handle(
    body: request::Body,
    features: ClientFeatures,
    context: &Context,
) -> Outcome {
    let result = match body {
        request::Body::Hello(_) => Err(Error {
            code: ErrorCode::Failed.into(),
            message: "the handshake of this connection is complete".to_owned(),
        }),
        request::Body::HostInfo(_) => Ok(host::info(context)),
        request::Body::HostStop(_) => Ok(host::stop(context)),
        request::Body::SessionCreate(request) => session::create(request, context).await,
        request::Body::SessionList(request) => Ok(session::list(request, context)),
        request::Body::SessionKill(request) => session::kill(&request, context).await,
        request::Body::SessionAttach(request) => {
            return match session::attach(&request, features, context).await {
                Ok(attachment) => Outcome::Attach(attachment),
                Err(error) => Outcome::Respond(response::Result::Error(error)),
            };
        }
        request::Body::SessionDetach(request) => session::detach(&request, context),
        request::Body::SessionInput(request) => session::input(request, context).await,
        request::Body::SessionResize(request) => session::resize(&request, context).await,
        request::Body::SessionStart(request) => session::start(&request, context).await,
        request::Body::SessionCapture(request) => session::capture(&request, context).await,
        request::Body::SessionSetRestorePolicy(request) => {
            session::set_restore_policy(&request, context)
        }
        request::Body::SessionMemory(_) => session_memory::memory(context).await,
        request::Body::ExtList(_) => Ok(ext::list(context)),
        request::Body::ExtInspect(request) => ext::inspect(request).await,
        request::Body::ExtInstall(request) => ext::install(request, context).await,
        request::Body::ExtRemove(request) => ext::remove(&request, context).await,
        request::Body::ExtSetEnabled(request) => ext::set_enabled(&request, context).await,
        request::Body::ExtCall(request) => ext::call(request, context).await,
        request::Body::AgentTree(request) => Ok(agent::tree(request, context)),
        request::Body::AgentExplain(request) => agent::explain(&request, context),
        request::Body::AgentSetStatus(request) => agent::set_status(&request, context),
        request::Body::EventsSubscribe(request) => {
            return Outcome::Subscribe(events::subscribe(&request, context));
        }
    };
    Outcome::Respond(result.unwrap_or_else(response::Result::Error))
}
