//! One subscription to the events of the host: the connection after an
//! `Events.Subscribe` response.

use std::collections::HashSet;
use std::sync::Arc;

use nomux_proto::wire::v1::{
    Envelope, Event, EventType, EventsSnapshot, EventsSubscribeRequest, EventsSubscribeResponse,
    envelope, event,
};
use nomux_transport::EnvelopeStream;
use tracing::debug;

use super::log::{EventLog, Read};
use crate::clock;
use crate::sessions::Sessions;
use crate::status::StatusRegistry;

/// The largest number of events that one read of the log takes. The
/// subscription sends them before it reads again.
const READ_BATCH: usize = 64;

/// A subscription that the daemon accepted and that did not send its
/// events yet.
pub(crate) struct Subscription {
    /// The `seq` of the next event for the client.
    next: u64,
    /// The types that the client asks for. Empty for each type.
    types: HashSet<EventType>,
    log: Arc<EventLog>,
    status: Arc<StatusRegistry>,
    sessions: Arc<Sessions>,
}

impl Subscription {
    pub(crate) fn new(
        request: &EventsSubscribeRequest,
        log: Arc<EventLog>,
        status: Arc<StatusRegistry>,
        sessions: Arc<Sessions>,
    ) -> Self {
        let next = match request.from_seq {
            0 => log.last_seq() + 1,
            from => from,
        };
        Self {
            next,
            types: request
                .types
                .iter()
                .filter_map(|value| EventType::try_from(*value).ok())
                .filter(|value| *value != EventType::Unspecified)
                .collect(),
            log,
            status,
            sessions,
        }
    }

    pub(crate) fn response(&self) -> EventsSubscribeResponse {
        EventsSubscribeResponse {
            last_seq: self.log.last_seq(),
        }
    }

    /// Sends the events to the client until it closes the connection.
    pub(crate) async fn run(mut self, mut stream: EnvelopeStream) {
        let mut newest = self.log.watch();
        loop {
            newest.borrow_and_update();
            if let Err(error) = self.send_available(&mut stream).await {
                debug!(%error, "a subscription ended with an error");
                return;
            }
            tokio::select! {
                // The client sends nothing after the subscription: anything
                // that it sends, also the end of the connection, ends it.
                _ = stream.receive() => return,
                changed = newest.changed() => if changed.is_err() {
                    return;
                },
            }
        }
    }

    /// Sends each event that the client did not get yet.
    async fn send_available(&mut self, stream: &mut EnvelopeStream) -> std::io::Result<()> {
        loop {
            let events = match self.log.read(self.next, READ_BATCH) {
                Read::Events(events) if events.is_empty() => return Ok(()),
                Read::Events(events) => events,
                Read::Lost => self.snapshot(),
            };
            for event in events {
                self.next = self.next.max(event.seq + 1);
                if self.accepts(&event) {
                    send(stream, event).await?;
                }
            }
        }
    }

    /// The events that give the client the state of the host now: an
    /// `EventsSnapshot` and the Agent run of each session. They have the
    /// `seq` of the newest event that they include.
    fn snapshot(&mut self) -> Vec<Event> {
        let ids: Vec<_> = self
            .sessions
            .list(false)
            .into_iter()
            .map(|listed| listed.record.id)
            .collect();
        let (last_seq, runs) = self.status.snapshot(&ids);
        let at_ms = clock::now().0;
        let marker = event::Body::Snapshot(EventsSnapshot {});
        let events: Vec<Event> = std::iter::once(marker)
            .chain(runs)
            .map(|body| Event {
                seq: last_seq,
                at_ms,
                body: Some(body),
            })
            .collect();
        self.next = last_seq + 1;
        events
    }

    fn accepts(&self, event: &Event) -> bool {
        let kind = match event.body {
            Some(event::Body::AgentRunChanged(_)) => EventType::AgentRunChanged,
            Some(event::Body::AgentRunRemoved(_)) => EventType::AgentRunRemoved,
            Some(event::Body::Snapshot(_)) | None => return true,
        };
        self.types.is_empty() || self.types.contains(&kind)
    }
}

async fn send(stream: &mut EnvelopeStream, event: Event) -> std::io::Result<()> {
    let envelope = Envelope {
        kind: Some(envelope::Kind::Event(event)),
    };
    stream.send(&envelope).await
}
