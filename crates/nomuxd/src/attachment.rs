//! One client that is attached to one session.
//!
//! An attach starts as the `Session.Attach` request on a control
//! connection. After the response, the connection carries terminal frames:
//! the screen and the output go to the client, and the input comes from the
//! client. `spec/frame.md` of the `proto` repository has the rules.

mod attached_clients;
mod frame_sink;
mod history_replay;
mod output_pump;

use std::sync::Arc;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::ring::RingReader;
use nomux_holder_protocol::wire::WindowSize;
use nomux_proto::frame::{EndReason, FrameKind};
use nomux_proto::wire::v1::{SessionAttachRequest, SessionAttachResponse};
use nomux_transport::{EnvelopeStream, FrameReader};
use tracing::debug;

pub(crate) use self::attached_clients::AttachedClients;
use self::frame_sink::FrameSink;
use self::history_replay::EarlierOutput;
use self::output_pump::{OutputPump, Start};
use crate::handshake::ClientFeatures;
use crate::holder::OutputFeed;
use crate::session_error::SessionError;
use crate::session_handle::SessionHandle;
use crate::session_journal::SessionJournal;
use crate::sessions::Sessions;

/// The session handle of the frames. A connection has one attach, thus the
/// handle is the same for each attach.
const HANDLE: u32 = 1;

/// An attach that the daemon accepted and that did not start its frames yet.
pub(crate) struct Attachment {
    session: Arc<SessionHandle>,
    feed: Arc<OutputFeed>,
    journal: SessionJournal,
    reader: RingReader,
    start: Start,
    features: ClientFeatures,
}

impl Attachment {
    /// Finds the session, starts its command again if the session is
    /// dormant, gives it the window size of the client and opens the read
    /// side of its holder.
    pub(crate) async fn prepare(
        request: &SessionAttachRequest,
        size: Option<WindowSize>,
        features: ClientFeatures,
        sessions: &Sessions,
    ) -> Result<Self, SessionError> {
        let attachable = sessions.attachable(&request.id, size).await?;
        let session = attachable.session;
        let cannot_read = |error| {
            SessionError::Failed(format!(
                "the daemon cannot read the terminal of session {}: {error}",
                request.id
            ))
        };
        let feed = session.output.get().await.map_err(cannot_read)?;
        let reader = feed.reader().map_err(cannot_read)?;
        let journal = sessions.live_journal(session.id(), Arc::clone(&feed));
        let start = start_of(request, feed.epoch(), attachable.restored_from, features);
        Ok(Self {
            session,
            feed,
            journal,
            reader,
            start,
            features,
        })
    }

    pub(crate) fn session_id(&self) -> &SessionId {
        self.session.id()
    }

    pub(crate) fn response(&self) -> SessionAttachResponse {
        SessionAttachResponse {
            handle: HANDLE,
            epoch: self.feed.epoch(),
        }
    }

    /// Serves the terminal channels of the attach until the session ends,
    /// a request detaches the client, or the client leaves.
    pub(crate) async fn run(self, connection: EnvelopeStream) {
        let (input, frames) = connection.into_frames();
        let mut detach = self.session.clients.join();
        let sink = FrameSink::new(frames, HANDLE, self.feed.epoch());
        let mut output = OutputPump::new(
            sink,
            self.feed,
            self.journal,
            self.reader,
            self.features.output_history,
            self.session.ended.clone(),
        );
        let result = tokio::select! {
            // The input comes first: it never waits for the output.
            biased;
            () = forward_input(input, &self.session) => Ok(()),
            () = detach.received() => output.end(EndReason::Detached).await,
            result = output.run(self.start) => result,
        };
        drop(detach);
        self.session.detached();
        if let Err(error) = result {
            debug!(%error, "an attach ended with an error");
        }
    }
}

/// Where the frames of an attach start: what the client has, and what the
/// client can read.
fn start_of(
    request: &SessionAttachRequest,
    epoch: u32,
    restored_from: Option<u32>,
    features: ClientFeatures,
) -> Start {
    match (request.epoch, restored_from) {
        (client, _) if client == epoch => Start::Resume(request.from_seq),
        _ if !features.output_history => Start::Screen,
        // The client that started the command again has no output of the
        // session: it gets the end of the output from before the restore.
        (0, Some(earlier)) => Start::Earlier {
            epoch: earlier,
            output: EarlierOutput::FromLastCheckpoint,
        },
        (client, _) if client != 0 && client < epoch => Start::Earlier {
            epoch: client,
            output: EarlierOutput::From(request.from_seq),
        },
        _ => Start::Screen,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn start(client_epoch: u32, restored_from: Option<u32>, output_history: bool) -> Start {
        let request = SessionAttachRequest {
            epoch: client_epoch,
            from_seq: 7,
            ..SessionAttachRequest::default()
        };
        start_of(
            &request,
            3,
            restored_from,
            ClientFeatures { output_history },
        )
    }

    #[test]
    fn only_a_client_that_reads_the_history_frames_gets_an_earlier_epoch() {
        let earlier = Start::Earlier {
            epoch: 2,
            output: EarlierOutput::From(7),
        };
        let before_restore = Start::Earlier {
            epoch: 2,
            output: EarlierOutput::FromLastCheckpoint,
        };

        assert_eq!(start(3, None, true), Start::Resume(7));
        assert_eq!(start(3, None, false), Start::Resume(7));
        assert_eq!(start(2, None, true), earlier);
        assert_eq!(start(2, Some(2), true), earlier);
        assert_eq!(start(0, Some(2), true), before_restore);
        // A client without a position gets old output only from the attach
        // that started the command again.
        assert_eq!(start(0, None, true), Start::Screen);
        // An epoch that the session does not have yet.
        assert_eq!(start(4, None, true), Start::Screen);
        for client_epoch in [0, 2] {
            assert_eq!(start(client_epoch, Some(2), false), Start::Screen);
        }
    }
}
