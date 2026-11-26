//! The agent socket: the second socket of the daemon, for the processes of
//! a sandboxed session. It takes only the requests with which a session
//! reports about itself: the handshake, `Agent.SetStatus` and `Hooks.Event`.
//! The two check the token of the session. No request controls a session.

use std::io;
use std::sync::Arc;

use nomux_proto::wire::v1::{
    Envelope, Error, ErrorCode, Request, Response, envelope, request, response,
};
use nomux_transport::{EnvelopeStream, LocalListener};
use tokio::task::spawn_local;
use tracing::{debug, warn};

use crate::handlers::{self, Context, Outcome};
use crate::handshake;

/// Accepts the connections of the agent socket until a stop is requested.
pub(crate) async fn serve_agent_socket(listener: LocalListener, context: Arc<Context>) {
    loop {
        tokio::select! {
            () = context.shutdown.requested() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    spawn_local(serve(stream, Arc::clone(&context)));
                }
                Err(error) => warn!(%error, "a connection of the agent socket was not accepted"),
            },
        }
    }
}

async fn serve(stream: EnvelopeStream, context: Arc<Context>) {
    if let Err(error) = converse(stream, &context).await {
        debug!(%error, "a connection of the agent socket ended with an error");
    }
}

async fn converse(mut stream: EnvelopeStream, context: &Context) -> io::Result<()> {
    let mut features = None;
    while let Some(Envelope {
        kind: Some(envelope::Kind::Request(Request { id, body })),
    }) = stream.receive().await?
    {
        let result = match (body, features) {
            (Some(request::Body::Hello(hello)), None) => match handshake::answer(&hello) {
                Ok(welcome) => {
                    features = Some(handshake::ClientFeatures::of(&hello));
                    response::Result::Welcome(welcome)
                }
                Err(error) => response::Result::Error(error),
            },
            (
                Some(body @ (request::Body::AgentSetStatus(_) | request::Body::HookEvent(_))),
                Some(features),
            ) => {
                let in_flight = context.in_flight_requests.begin().await;
                let outcome = handlers::handle(body, features, context).await;
                drop(in_flight);
                match outcome {
                    Outcome::Respond(result) => result,
                    _ => response::Result::Error(refused()),
                }
            }
            _ => response::Result::Error(refused()),
        };
        let refused_handshake = features.is_none();
        stream
            .send(&Envelope {
                kind: Some(envelope::Kind::Response(Response {
                    id,
                    result: Some(result),
                })),
            })
            .await?;
        if refused_handshake {
            return Ok(());
        }
    }
    Ok(())
}

fn refused() -> Error {
    Error {
        code: ErrorCode::PermissionDenied.into(),
        message: "a sandboxed session can only report its status and its hook events: the \
                  agent socket takes no other request"
            .to_owned(),
    }
}
