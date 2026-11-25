//! The request handlers: each one changes a request of the protocol into a
//! call of the daemon and the result into a response of the protocol.

mod host;
mod session;

use nomux_proto::wire::v1::{Error, ErrorCode, request, response};

use crate::in_flight_requests::InFlightRequests;
use crate::sessions::Sessions;
use crate::shutdown::Shutdown;

/// What the handlers of all connections share.
pub(crate) struct Context {
    pub(crate) sessions: Sessions,
    pub(crate) shutdown: Shutdown,
    pub(crate) in_flight_requests: InFlightRequests,
}

/// Does one request and gives the result for the response.
pub(crate) async fn handle(body: request::Body, context: &Context) -> response::Result {
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
    };
    result.unwrap_or_else(response::Result::Error)
}
