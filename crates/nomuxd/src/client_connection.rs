//! One connection of a client: the handshake, then the requests.

use std::io;
use std::sync::Arc;

use nomux_proto::wire::v1::{
    Envelope, Error, ErrorCode, Request, Response, envelope, request, response,
};
use nomux_transport::EnvelopeStream;
use tracing::debug;

use crate::handlers::{self, Context};
use crate::handshake;

/// Serves one connection until the client closes it or breaks the protocol.
pub(crate) async fn serve(mut stream: EnvelopeStream, context: Arc<Context>) {
    if let Err(error) = converse(&mut stream, &context).await {
        debug!(%error, "a client connection ended with an error");
    }
}

async fn converse(stream: &mut EnvelopeStream, context: &Context) -> io::Result<()> {
    if !shake_hands(stream).await? {
        return Ok(());
    }
    // The requests of one connection are answered in their sequence.
    while let Some(request) = next_request(stream).await? {
        let _in_flight = context.in_flight_requests.begin().await;
        let result = match request.body {
            Some(body) => handlers::handle(body, context).await,
            None => response::Result::Error(failed("the daemon does not know this request")),
        };
        respond(stream, request.id, result).await?;
    }
    Ok(())
}

/// Answers the first request, which must be the `Hello`. Returns `false` if
/// the connection must close: the client is not compatible, or it did not
/// start with the handshake.
async fn shake_hands(stream: &mut EnvelopeStream) -> io::Result<bool> {
    let Some(request) = next_request(stream).await? else {
        return Ok(false);
    };
    let answer = match &request.body {
        Some(request::Body::Hello(hello)) => handshake::answer(hello),
        _ => Err(failed("the first request of a connection must be Hello")),
    };
    let accepted = answer.is_ok();
    let result = match answer {
        Ok(welcome) => response::Result::Welcome(welcome),
        Err(error) => response::Result::Error(error),
    };
    respond(stream, request.id, result).await?;
    Ok(accepted)
}

/// Reads the next request. Returns `None` if the client closed the
/// connection.
async fn next_request(stream: &mut EnvelopeStream) -> io::Result<Option<Request>> {
    match stream.receive().await? {
        None => Ok(None),
        Some(Envelope {
            kind: Some(envelope::Kind::Request(request)),
        }) => Ok(Some(request)),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a client sends only requests",
        )),
    }
}

async fn respond(stream: &mut EnvelopeStream, id: u64, result: response::Result) -> io::Result<()> {
    let response = Envelope {
        kind: Some(envelope::Kind::Response(Response {
            id,
            result: Some(result),
        })),
    };
    stream.send(&response).await
}

fn failed(message: &str) -> Error {
    Error {
        code: ErrorCode::Failed.into(),
        message: message.to_owned(),
    }
}
