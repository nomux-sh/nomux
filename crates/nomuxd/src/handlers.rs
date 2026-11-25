//! The request handlers: each one changes a request of the protocol into a
//! call of the daemon and the result into a response of the protocol.

mod host;
mod session;

use nomux_proto::wire::v1::{Error, ErrorCode, request, response};

use crate::attachment::Attachment;
use crate::in_flight_requests::InFlightRequests;
use crate::sessions::Sessions;
use crate::shutdown::Shutdown;

/// What the handlers of all connections share.
pub(crate) struct Context {
    pub(crate) sessions: Sessions,
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
}

/// Does one request.
pub(crate) async fn handle(body: request::Body, context: &Context) -> Outcome {
    let result = match body {
        request::Body::Hello(_) => Err(Error {
            code: ErrorCode::Failed.into(),
            message: "the handshake of this connection is complete".to_owned(),
        }),
        request::Body::HostInfo(_) => Ok(host::info(context)),
        request::Body::HostStop(_) => Ok(host::stop(context)),
        request::Body::SessionCreate(request) => session::create(request, context).await,
        request::Body::SessionList(_) => Ok(session::list(context)),
        request::Body::SessionKill(request) => session::kill(&request, context).await,
        request::Body::SessionAttach(request) => {
            return match session::attach(&request, context).await {
                Ok(attachment) => Outcome::Attach(attachment),
                Err(error) => Outcome::Respond(response::Result::Error(error)),
            };
        }
        request::Body::SessionDetach(request) => session::detach(&request, context),
        request::Body::SessionInput(request) => session::input(request, context).await,
        request::Body::SessionResize(request) => session::resize(&request, context).await,
    };
    Outcome::Respond(result.unwrap_or_else(response::Result::Error))
}
