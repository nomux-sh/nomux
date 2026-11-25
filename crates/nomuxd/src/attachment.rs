//! One client that is attached to one session.
//!
//! An attach starts as the `Session.Attach` request on a control
//! connection. After the response, the connection carries terminal frames:
//! the screen and the output go to the client, and the input comes from the
//! client. `spec/frame.md` of the `proto` repository has the rules.

mod attached_clients;
mod output_pump;

use std::sync::Arc;

use nomux_holder_protocol::ring::RingReader;
use nomux_holder_protocol::wire::WindowSize;
use nomux_proto::frame::{EndReason, FIRST_EPOCH, FrameKind};
use nomux_proto::wire::v1::{SessionAttachRequest, SessionAttachResponse};
use nomux_transport::{EnvelopeStream, FrameReader};
use tracing::debug;

pub(crate) use self::attached_clients::AttachedClients;
use self::output_pump::OutputPump;
use crate::holder::OutputFeed;
use crate::session_error::SessionError;
use crate::session_handle::SessionHandle;
use crate::sessions::Sessions;

/// The session handle of the frames. A connection has one attach, thus the
/// handle is the same for each attach.
const HANDLE: u32 = 1;

/// An attach that the daemon accepted and that did not start its frames yet.
pub(crate) struct Attachment {
    session: Arc<SessionHandle>,
    feed: Arc<OutputFeed>,
    reader: RingReader,
    /// The position from which the client asks for the output, if it has
    /// the output before that position.
    resume: Option<u64>,
}

impl Attachment {
    /// Finds the session, gives it the window size of the client and opens
    /// the read side of its holder.
    pub(crate) async fn prepare(
        request: &SessionAttachRequest,
        size: Option<WindowSize>,
        sessions: &Sessions,
    ) -> Result<Self, SessionError> {
        let session = sessions.find_reachable(&request.id)?;
        if let Some(size) = size {
            session.resize(size).await?;
        }
        let cannot_read = |error| {
            SessionError::Failed(format!(
                "the daemon cannot read the terminal of session {}: {error}",
                request.id
            ))
        };
        let feed = session.output.get().await.map_err(cannot_read)?;
        let reader = feed.reader().map_err(cannot_read)?;
        Ok(Self {
            session,
            feed,
            reader,
            resume: (request.epoch == FIRST_EPOCH).then_some(request.from_seq),
        })
    }

    #[expect(
        clippy::unused_self,
        reason = "only an attach that the daemon accepted has a response"
    )]
    pub(crate) fn response(&self) -> SessionAttachResponse {
        SessionAttachResponse {
            handle: HANDLE,
            epoch: FIRST_EPOCH,
        }
    }

    /// Serves the terminal channels of the attach until the session ends,
    /// a request detaches the client, or the client leaves.
    pub(crate) async fn run(self, connection: EnvelopeStream) {
        let (input, frames) = connection.into_frames();
        let mut detach = self.session.clients.join();
        let mut output = OutputPump::new(
            frames,
            HANDLE,
            self.feed,
            self.reader,
            self.session.ended.clone(),
        );
        let result = tokio::select! {
            // The input comes first: it never waits for the output.
            biased;
            () = forward_input(input, &self.session) => Ok(()),
            () = detach.received() => output.end(EndReason::Detached).await,
            result = output.run(self.resume) => result,
        };
        if let Err(error) = result {
            debug!(%error, "an attach ended with an error");
        }
    }
}

/// The input channel of one attach. Returns when the client leaves.
async fn forward_input(mut frames: FrameReader, session: &SessionHandle) {
    while let Ok(Some(header)) = frames.receive().await {
        if header.kind == FrameKind::Input && header.session == HANDLE {
            session.forward_input(frames.payload().to_vec()).await;
        }
    }
}
