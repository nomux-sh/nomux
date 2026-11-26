//! One connection of a client: the handshake, then the requests.

use std::io;
use std::sync::Arc;

use nomux_proto::wire::v1::{
    Envelope, Error, ErrorCode, Request, Response, envelope, request, response,
};
use nomux_transport::EnvelopeStream;
use tracing::debug;

use crate::handlers::{self, Context, Outcome};
use crate::handshake::{self, ClientFeatures};

/// Serves one connection until the client closes it or breaks the protocol.
pub(crate) async fn serve(stream: EnvelopeStream, context: Arc<Context>) {
    if let Err(error) = converse(stream, &context).await {
        debug!(%error, "a client connection ended with an error");
    }
}

async fn converse(mut stream: EnvelopeStream, context: &Context) -> io::Result<()> {
    let Some(features) = shake_hands(&mut stream).await? else {
        return Ok(());
    };
    // The requests of one connection are answered in their sequence.
    while let Some(request) = next_request(&mut stream).await? {
        let in_flight = context.in_flight_requests.begin().await;
        let outcome = match request.body {
            Some(body) => handlers::handle(body, features, context).await,
            None => Outcome::Respond(response::Result::Error(failed(
                "the daemon does not know this request",
            ))),
        };
        match outcome {
            Outcome::Respond(result) => respond(&mut stream, request.id, result).await?,
            Outcome::Attach(attachment) => {
                let accepted = response::Result::SessionAttach(attachment.response());
                respond(&mut stream, request.id, accepted).await?;
                // The request is complete. The attach is not a request: it
                // ends when the daemon closes the connections.
                drop(in_flight);
                attachment.run(stream).await;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Answers the first request, which must be the `Hello`. Returns the
/// features of the connection, or `None` if the connection must close: the
/// client is not compatible, or it did not start with the handshake.
async fn shake_hands(stream: &mut EnvelopeStream) -> io::Result<Option<ClientFeatures>> {
    let Some(request) = next_request(stream).await? else {
        return Ok(None);
    };
    let answer = match &request.body {
        Some(request::Body::Hello(hello)) => {
            handshake::answer(hello).map(|welcome| (welcome, ClientFeatures::of(hello)))
        }
        _ => Err(failed("the first request of a connection must be Hello")),
    };
    let (result, features) = match answer {
        Ok((welcome, features)) => (response::Result::Welcome(welcome), Some(features)),
        Err(error) => (response::Result::Error(error), None),
    };
    respond(stream, request.id, result).await?;
    Ok(features)
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
