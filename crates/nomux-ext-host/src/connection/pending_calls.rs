//! The calls from the host to one extension that have no response yet.

use std::collections::HashMap;
use std::time::Duration;

use nomux_extension::wire::v1::response;
use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::call_error::CallError;
use crate::host_services::ServiceError;

/// The result of one call, as the caller gets it.
pub(crate) type CallResult = Result<Vec<u8>, CallError>;

enum Pending {
    /// A caller waits for the result.
    Waiting(oneshot::Sender<CallResult>),
    /// The host cancelled the call. The extension must answer before the
    /// deadline, because it does not start its subsequent call before.
    Cancelled { answer_by: Instant },
}

#[derive(Default)]
pub(super) struct PendingCalls {
    calls: HashMap<u64, Pending>,
}

impl PendingCalls {
    pub(super) fn add(&mut self, id: u64, reply: oneshot::Sender<CallResult>) {
        self.calls.insert(id, Pending::Waiting(reply));
    }

    /// Records that the host cancelled a call. Returns `false` if the call
    /// already has its response.
    pub(super) fn cancel(&mut self, id: u64, limit: Duration) -> bool {
        let Some(pending) = self.calls.get_mut(&id) else {
            return false;
        };
        *pending = Pending::Cancelled {
            answer_by: Instant::now() + limit,
        };
        true
    }

    /// Gives the response of the extension to the caller. A response for a
    /// call that the host does not know is dropped.
    pub(super) fn resolve(&mut self, id: u64, result: Option<response::Result>) {
        if let Some(Pending::Waiting(reply)) = self.calls.remove(&id) {
            // A caller that is gone stopped waiting.
            let _ = reply.send(call_result(result));
        }
    }

    /// Fails each call: the extension stops, or its process ended. Returns
    /// the calls for which a caller waited.
    pub(super) fn fail_all(&mut self, error: &CallError) -> Vec<u64> {
        let mut waiting = Vec::new();
        for (id, pending) in self.calls.drain() {
            if let Pending::Waiting(reply) = pending {
                // A caller that is gone stopped waiting.
                let _ = reply.send(Err(error.clone()));
                waiting.push(id);
            }
        }
        waiting
    }

    pub(super) fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// The earliest time at which a cancelled call must have its response.
    pub(super) fn cancel_deadline(&self) -> Option<Instant> {
        let deadlines = self.calls.values().filter_map(|pending| match pending {
            Pending::Waiting(_) => None,
            Pending::Cancelled { answer_by } => Some(*answer_by),
        });
        deadlines.min()
    }
}

fn call_result(result: Option<response::Result>) -> CallResult {
    match result {
        Some(response::Result::Call(reply)) => Ok(reply.payload),
        Some(response::Result::Error(error)) => Err(CallError::Answered(error.into())),
        _ => Err(CallError::Answered(ServiceError::failed(
            "the extension did not answer with a call result",
        ))),
    }
}
