//! The event log: the last events of the host, in memory.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use nomux_proto::wire::v1::{Event, event};
use prost::Message;
use tokio::sync::watch;

use crate::clock;

/// The largest number of events in the log.
const MAX_EVENTS: usize = 1024;
/// The largest number of bytes of the encoded events in the log. An event
/// has the Agent run of a session, which a program of the session makes
/// large, thus the number of events alone does not limit the memory.
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// What a read of the log gives.
#[derive(Debug, PartialEq)]
pub(crate) enum Read {
    /// The events from the position on. Empty if no event is there yet.
    Events(Vec<Event>),
    /// The log does not have the event at the position any more, or never
    /// had it: the daemon started again after it.
    Lost,
}

/// The last events of the host. A subscriber reads them from its own
/// position and is told when an event comes. The log keeps no queue for a
/// subscriber.
pub(crate) struct EventLog {
    inner: Mutex<Inner>,
    /// The `seq` of the newest event.
    newest: watch::Sender<u64>,
}

#[derive(Default)]
struct Inner {
    events: VecDeque<Event>,
    bytes: usize,
    last_seq: u64,
}

impl Default for EventLog {
    fn default() -> Self {
        Self {
            inner: Mutex::default(),
            newest: watch::Sender::new(0),
        }
    }
}

impl EventLog {
    /// Adds an event with `body`. It gets the next `seq`.
    pub(crate) fn publish(&self, body: event::Body) {
        let mut inner = self.inner();
        inner.last_seq += 1;
        let event = Event {
            seq: inner.last_seq,
            at_ms: clock::now().0,
            body: Some(body),
        };
        inner.bytes += event.encoded_len();
        inner.events.push_back(event);
        while inner.events.len() > MAX_EVENTS || inner.bytes > MAX_BYTES {
            let Some(oldest) = inner.events.pop_front() else {
                break;
            };
            inner.bytes -= oldest.encoded_len();
        }
        self.newest.send_replace(inner.last_seq);
    }

    /// The `seq` of the newest event. 0 if no event was published.
    pub(crate) fn last_seq(&self) -> u64 {
        self.inner().last_seq
    }

    /// `limit` events at most from the event `from` on.
    pub(crate) fn read(&self, from: u64, limit: usize) -> Read {
        let inner = self.inner();
        let next = inner.last_seq + 1;
        let oldest = next - inner.events.len() as u64;
        if from < oldest || from > next {
            return Read::Lost;
        }
        let skip = usize::try_from(from - oldest).expect("the log has fewer events than usize");
        Read::Events(
            inner
                .events
                .iter()
                .skip(skip)
                .take(limit)
                .cloned()
                .collect(),
        )
    }

    /// Tells the receiver the `seq` of each new event.
    pub(crate) fn watch(&self) -> watch::Receiver<u64> {
        self.newest.subscribe()
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        // No code panics while it holds the lock, thus the log is whole.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use nomux_proto::wire::v1::AgentRunRemoved;

    use super::*;

    fn removed(session_id: &str) -> event::Body {
        event::Body::AgentRunRemoved(AgentRunRemoved {
            session_id: session_id.to_owned(),
        })
    }

    fn seqs(read: Read) -> Vec<u64> {
        match read {
            Read::Events(events) => events.iter().map(|event| event.seq).collect(),
            Read::Lost => panic!("the log lost the events"),
        }
    }

    #[test]
    fn a_reader_gets_the_events_from_its_position_and_nothing_twice() {
        let log = EventLog::default();
        for session in ["a", "b", "c"] {
            log.publish(removed(session));
        }

        assert_eq!(seqs(log.read(1, 10)), [1, 2, 3]);
        assert_eq!(seqs(log.read(2, 1)), [2]);
        assert_eq!(seqs(log.read(4, 10)), Vec::<u64>::new());
        // A position after the next event: the daemon started again.
        assert_eq!(log.read(5, 10), Read::Lost);
    }

    #[test]
    fn the_log_keeps_the_last_events_only() {
        let log = EventLog::default();
        for _ in 0..MAX_EVENTS + 2 {
            log.publish(removed("a"));
        }

        assert_eq!(log.read(2, 1), Read::Lost);
        assert_eq!(seqs(log.read(3, 1)), [3]);
        assert_eq!(log.last_seq(), MAX_EVENTS as u64 + 2);
    }

    #[test]
    fn large_events_are_limited_by_their_bytes() {
        let log = EventLog::default();
        let large = "s".repeat(MAX_BYTES / 4);
        for _ in 0..5 {
            log.publish(removed(&large));
        }

        assert_eq!(log.read(1, 10), Read::Lost);
        assert_eq!(seqs(log.read(3, 10)), [3, 4, 5]);
    }
}
