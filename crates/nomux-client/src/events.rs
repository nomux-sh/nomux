//! The events of a host: a connection that subscribed to them.

use std::io;

use nomux_proto::wire::v1::{
    self as wire, EventsSubscribeRequest, envelope, event, request, response,
};
use nomux_transport::EnvelopeStream;

#[cfg(doc)]
use crate::Workspace;
use crate::client::unexpected_result;
use crate::workspace::change_of;
use crate::{AgentRun, Client, ClientError, WorkspaceChange};

/// A type of event, for the filter of a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// [`EventBody::AgentRunChanged`].
    AgentRunChanged,
    /// [`EventBody::AgentRunRemoved`].
    AgentRunRemoved,
    /// [`EventBody::ExtensionEvent`].
    ExtensionEvent,
    /// [`EventBody::WorkspaceChanged`].
    WorkspaceChanged,
}

/// One event of a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEvent {
    /// The position of the event in the event log of the daemon.
    pub seq: u64,
    /// When the event occurred: milliseconds after the Unix epoch.
    pub at_ms: u64,
    /// What occurred.
    pub body: EventBody,
}

/// What occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventBody {
    /// The status of a session or of its Child work changed. The value is
    /// the Agent run after the change.
    AgentRunChanged(Box<AgentRun>),
    /// A session ended or was removed: it has no status any more.
    AgentRunRemoved {
        /// The identifier of the session.
        session_id: String,
    },
    /// The daemon part of an extension sent an event.
    ExtensionEvent {
        /// The identifier of the extension.
        extension_id: String,
        /// The name that the extension gave to the event.
        name: String,
        /// The payload. The extension selects its encoding.
        payload: Vec<u8>,
    },
    /// The workspace tree changed. [`Workspace::apply`] applies the change
    /// to a copy of the tree.
    WorkspaceChanged {
        /// The revision of the tree after the change.
        revision: u64,
        /// The change.
        change: WorkspaceChange,
    },
    /// The daemon does not have the events that the subscription asked
    /// for. The state of the host now follows: discard the state that you
    /// have.
    Snapshot,
    /// An event that this client does not know.
    Unknown,
}

/// The events of a host, from a connection that subscribed to them.
#[derive(Debug)]
pub struct EventStream {
    stream: EnvelopeStream,
    /// The `seq` of the newest event of the host when the subscription
    /// started.
    pub last_seq: u64,
}

impl Client {
    /// Changes this connection into a stream of the events of the host.
    /// `from_seq` is the `seq` of the first event to get: 0 for only the
    /// events after the subscription. With `types`, only events of these
    /// types come, and the snapshot events.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn subscribe_events(
        mut self,
        from_seq: u64,
        types: &[EventType],
    ) -> Result<EventStream, ClientError> {
        let request = EventsSubscribeRequest {
            from_seq,
            types: types
                .iter()
                .map(|kind| wire::EventType::from(*kind).into())
                .collect(),
        };
        match self.call(request::Body::EventsSubscribe(request)).await? {
            response::Result::EventsSubscribe(subscribed) => Ok(EventStream {
                stream: self.into_stream(),
                last_seq: subscribed.last_seq,
            }),
            _ => Err(unexpected_result()),
        }
    }
}

impl EventStream {
    /// Waits for the next event. Returns `None` if the daemon closed the
    /// connection, for example because it stopped.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or if the daemon sends a
    /// message that is not an event.
    pub async fn next(&mut self) -> Result<Option<HostEvent>, ClientError> {
        let Some(envelope) = self
            .stream
            .receive()
            .await
            .map_err(ClientError::Connection)?
        else {
            return Ok(None);
        };
        let Some(envelope::Kind::Event(event)) = envelope.kind else {
            return Err(ClientError::Connection(io::Error::new(
                io::ErrorKind::InvalidData,
                "the daemon sent a message that is not an event",
            )));
        };
        Ok(Some(event.into()))
    }
}

impl From<EventType> for wire::EventType {
    fn from(kind: EventType) -> Self {
        match kind {
            EventType::AgentRunChanged => Self::AgentRunChanged,
            EventType::AgentRunRemoved => Self::AgentRunRemoved,
            EventType::ExtensionEvent => Self::ExtensionEvent,
            EventType::WorkspaceChanged => Self::WorkspaceChanged,
        }
    }
}

impl From<wire::Event> for HostEvent {
    fn from(event: wire::Event) -> Self {
        let body = match event.body {
            Some(event::Body::AgentRunChanged(changed)) => {
                changed.run.map_or(EventBody::Unknown, |run| {
                    EventBody::AgentRunChanged(Box::new(run.into()))
                })
            }
            Some(event::Body::AgentRunRemoved(removed)) => EventBody::AgentRunRemoved {
                session_id: removed.session_id,
            },
            Some(event::Body::ExtensionEvent(extension_event)) => EventBody::ExtensionEvent {
                extension_id: extension_event.extension_id,
                name: extension_event.name,
                payload: extension_event.payload,
            },
            Some(event::Body::WorkspaceChanged(changed)) => EventBody::WorkspaceChanged {
                revision: changed.revision,
                change: change_of(changed.change),
            },
            Some(event::Body::Snapshot(_)) => EventBody::Snapshot,
            None => EventBody::Unknown,
        };
        Self {
            seq: event.seq,
            at_ms: event.at_ms,
            body,
        }
    }
}
