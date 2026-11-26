//! The events of a host: a connection that subscribed to them.

use std::io;

use nomux_proto::wire::v1::{
    self as wire, EventsSubscribeRequest, envelope, event, request, response,
};
use nomux_transport::EnvelopeStream;

#[cfg(doc)]
use crate::Workspace;
use crate::client::unexpected_result;
use crate::hibernation::reason_of;
use crate::session_memory::measure_of;
use crate::workspace::change_of;
use crate::{AgentRun, Client, ClientError, HibernationReason, MemoryMeasure, WorkspaceChange};

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
    /// [`EventBody::SessionHibernated`].
    SessionHibernated,
    /// [`EventBody::SessionWoken`].
    SessionWoken,
    /// [`EventBody::SessionWakeFailed`].
    SessionWakeFailed,
    /// [`EventBody::SessionMemoryLimitReached`].
    SessionMemoryLimitReached,
}

/// The daemon hibernated a session: why, and the numbers of the trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HibernatedEvent {
    /// The identifier of the session.
    pub session_id: String,
    /// Why the daemon hibernated the session.
    pub reason: HibernationReason,
    /// The memory that the hibernation gave back, in bytes.
    pub freed_bytes: u64,
    /// How the daemon measured `freed_bytes` and `memory_bytes`.
    pub measure: MemoryMeasure,
    /// [`HibernationReason::Count`]: the live agent sessions and their
    /// limit.
    pub live_agents: Option<(u32, u32)>,
    /// [`HibernationReason::SessionMemory`] and
    /// [`HibernationReason::TotalMemory`]: the measured memory and the
    /// limit, in bytes.
    pub memory: Option<(u64, u64)>,
    /// [`HibernationReason::MemoryPressure`]: the stall time and the window
    /// of the report, in milliseconds.
    pub pressure: Option<(u32, u32)>,
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
    /// The daemon hibernated a session.
    SessionHibernated(HibernatedEvent),
    /// A wake started the resume command of a hibernated session.
    SessionWoken {
        /// The identifier of the session.
        session_id: String,
        /// The command that the wake started.
        resume_command: String,
    },
    /// A wake of a hibernated session failed. The session stays
    /// hibernated.
    SessionWakeFailed {
        /// The identifier of the session.
        session_id: String,
        /// Why the wake failed.
        error: String,
        /// The command that the user can run by hand.
        resume_command: String,
    },
    /// The operating system ended a session at its memory limit.
    SessionMemoryLimitReached {
        /// The identifier of the session.
        session_id: String,
        /// The limit, in bytes.
        limit_bytes: u64,
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
            EventType::SessionHibernated => Self::SessionHibernated,
            EventType::SessionWoken => Self::SessionWoken,
            EventType::SessionWakeFailed => Self::SessionWakeFailed,
            EventType::SessionMemoryLimitReached => Self::SessionMemoryLimitReached,
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
            Some(event::Body::SessionHibernated(hibernated)) => {
                EventBody::SessionHibernated(hibernated.into())
            }
            Some(event::Body::SessionWoken(woken)) => EventBody::SessionWoken {
                session_id: woken.session_id,
                resume_command: woken.resume_command,
            },
            Some(event::Body::SessionWakeFailed(failed)) => EventBody::SessionWakeFailed {
                session_id: failed.session_id,
                error: failed.error,
                resume_command: failed.resume_command,
            },
            Some(event::Body::SessionMemoryLimitReached(reached)) => {
                EventBody::SessionMemoryLimitReached {
                    session_id: reached.session_id,
                    limit_bytes: reached.limit_bytes,
                }
            }
            Some(event::Body::Snapshot(_)) => EventBody::Snapshot,
            // The events of the worktrees come to the clients in a later
            // version.
            Some(event::Body::WorktreeChanged(_) | event::Body::WorktreeDiskPressure(_)) | None => {
                EventBody::Unknown
            }
        };
        Self {
            seq: event.seq,
            at_ms: event.at_ms,
            body,
        }
    }
}

impl From<wire::SessionHibernated> for HibernatedEvent {
    fn from(event: wire::SessionHibernated) -> Self {
        let reason = reason_of(event.reason);
        Self {
            session_id: event.session_id,
            freed_bytes: event.freed_bytes,
            measure: measure_of(event.measure),
            live_agents: (reason == HibernationReason::Count)
                .then_some((event.live_agents, event.max_live_agents)),
            memory: matches!(
                reason,
                HibernationReason::SessionMemory | HibernationReason::TotalMemory
            )
            .then_some((event.memory_bytes, event.memory_limit_bytes)),
            pressure: (reason == HibernationReason::MemoryPressure)
                .then_some((event.pressure_stall_ms, event.pressure_window_ms)),
            reason,
        }
    }
}
