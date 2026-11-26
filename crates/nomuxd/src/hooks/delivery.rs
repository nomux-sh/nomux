//! The queue of the hook events that the adapters did not take yet.
//!
//! The daemon answers a hook at once and gives its event to the adapter
//! later: a hook must not wait for an extension that starts or hangs. One
//! task gives the events to the adapters in the sequence of their arrival,
//! thus an adapter sees the events of a tool in the sequence of the tool.
//! The task exists only while the queue has events.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_ext_host::ExtensionHost;
use nomux_proto::wire::v1::HookEvent;
use prost::Message;
use tokio::task::spawn_local;
use tracing::debug;

use super::adapters::HOOK_METHOD;

/// The largest number of events in the queue.
const MAX_QUEUED_EVENTS: usize = 256;
/// The largest number of bytes of the events in the queue. A hook event has
/// 256 KiB of data at most, thus the number of events alone does not limit
/// the memory.
const MAX_QUEUED_BYTES: usize = 4 * 1024 * 1024;

/// Gives the hook events to the adapters.
pub(crate) struct HookDelivery {
    host: ExtensionHost,
    queue: Mutex<Queue>,
}

#[derive(Default)]
struct Queue {
    events: VecDeque<Queued>,
    bytes: usize,
    /// `true` while the task that gives the events to the adapters runs.
    draining: bool,
}

struct Queued {
    extension_id: String,
    /// The encoded `HookEvent`.
    payload: Vec<u8>,
}

impl HookDelivery {
    pub(crate) fn new(host: ExtensionHost) -> Self {
        Self {
            host,
            queue: Mutex::default(),
        }
    }

    /// Puts `event` into the queue for the adapter `extension_id`. Returns
    /// `false` if the queue is full: the event is dropped.
    pub(crate) fn deliver(self: &Arc<Self>, extension_id: String, event: &HookEvent) -> bool {
        let payload = event.encode_to_vec();
        let mut queue = self.queue();
        let full = queue.events.len() >= MAX_QUEUED_EVENTS
            || queue.bytes + payload.len() > MAX_QUEUED_BYTES;
        if full {
            return false;
        }
        queue.bytes += payload.len();
        queue.events.push_back(Queued {
            extension_id,
            payload,
        });
        if !queue.draining {
            queue.draining = true;
            spawn_local(Arc::clone(self).drain());
        }
        true
    }

    /// Calls the method `hook` of the adapter of each event, one event at a
    /// time, until the queue is empty. The first call starts an adapter.
    async fn drain(self: Arc<Self>) {
        while let Some(event) = self.next() {
            let called = self
                .host
                .call(&event.extension_id, HOOK_METHOD, event.payload)
                .await;
            if let Err(error) = called {
                debug!(
                    extension = %event.extension_id,
                    %error,
                    "the adapter did not take a hook event"
                );
            }
        }
    }

    /// The oldest event of the queue. `None` if the queue is empty: the task
    /// ends then.
    fn next(&self) -> Option<Queued> {
        let mut queue = self.queue();
        let Some(event) = queue.events.pop_front() else {
            queue.draining = false;
            return None;
        };
        queue.bytes -= event.payload.len();
        Some(event)
    }

    fn queue(&self) -> MutexGuard<'_, Queue> {
        // No code panics while it holds the lock, thus the queue is whole.
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
