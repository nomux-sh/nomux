//! The side of a connection that the callers use.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use nomux_extension::frame::MAX_ENVELOPE_LENGTH;
use nomux_extension::wire::v1::request::Body;
use nomux_extension::wire::v1::{CallRequest, Envelope};
use prost::Message;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use super::envelopes;
use super::pending_calls::CallResult;
use crate::call_error::CallError;

/// What a caller asks the task of a connection to do.
pub(super) enum Command {
    /// Send a call to the extension and give its response to `reply`.
    Call {
        id: u64,
        envelope: Envelope,
        reply: oneshot::Sender<CallResult>,
    },
    /// Tell the extension to stop the work of a call.
    Cancel { id: u64 },
    /// Stop the extension. `done` closes when the process has ended.
    Stop { done: oneshot::Sender<()> },
}

/// The request numbers of the host on one connection. They start at 1: the
/// protocol does not permit 0.
#[derive(Debug, Clone, Default)]
pub(super) struct RequestIds(Arc<AtomicU64>);

impl RequestIds {
    pub(super) fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Sends commands to the task that owns the process of an extension.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionHandle {
    commands: mpsc::UnboundedSender<Command>,
    request_ids: RequestIds,
}

impl ConnectionHandle {
    pub(super) fn new(commands: mpsc::UnboundedSender<Command>, request_ids: RequestIds) -> Self {
        Self {
            commands,
            request_ids,
        }
    }

    /// Gives one call to the task of the connection.
    pub(crate) fn begin_call(
        &self,
        method: &str,
        payload: Vec<u8>,
    ) -> Result<InFlightCall, CallError> {
        let id = self.request_ids.next();
        let call = CallRequest {
            method: method.to_owned(),
            payload,
        };
        let envelope = envelopes::request(id, Body::Call(call));
        let length = envelope.encoded_len();
        if length > MAX_ENVELOPE_LENGTH {
            return Err(CallError::TooLarge { length });
        }
        let (reply, result) = oneshot::channel();
        let command = Command::Call {
            id,
            envelope,
            reply,
        };
        self.commands
            .send(command)
            .map_err(|_| CallError::Stopped)?;
        Ok(InFlightCall {
            id,
            result,
            commands: self.commands.clone(),
            is_answered: false,
        })
    }

    /// Returns `true` after the task of the connection ended.
    pub(crate) fn is_closed(&self) -> bool {
        self.commands.is_closed()
    }

    /// Tells the task to stop the extension. The result completes when the
    /// process has ended.
    pub(crate) fn stop(&self) -> oneshot::Receiver<()> {
        let (done, stopped) = oneshot::channel();
        // A task that is gone already stopped the extension, and `stopped`
        // completes immediately.
        let _ = self.commands.send(Command::Stop { done });
        stopped
    }
}

/// One call that the extension has. If the caller drops it before the
/// response, the host cancels the call.
#[derive(Debug)]
pub(crate) struct InFlightCall {
    id: u64,
    result: oneshot::Receiver<CallResult>,
    commands: mpsc::UnboundedSender<Command>,
    is_answered: bool,
}

impl InFlightCall {
    /// Waits for the response. After `limit`, the call is cancelled.
    pub(crate) async fn result(mut self, limit: Duration) -> CallResult {
        let Ok(received) = timeout(limit, &mut self.result).await else {
            return Err(CallError::Timeout);
        };
        self.is_answered = true;
        received.unwrap_or(Err(CallError::Stopped))
    }
}

impl Drop for InFlightCall {
    fn drop(&mut self) {
        if !self.is_answered {
            // A task that is gone has no call to cancel.
            let _ = self.commands.send(Command::Cancel { id: self.id });
        }
    }
}
