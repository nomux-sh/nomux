//! The calls of host services that one extension sent and that have no
//! response yet.

use std::collections::HashMap;
use std::sync::Arc;

use nomux_extension::ErrorCode;
use nomux_extension::frame::MAX_ENVELOPE_LENGTH;
use nomux_extension::wire::v1::{CallRequest, CallResponse, Envelope, response};
use prost::Message;
use tokio::task::{self, AbortHandle, JoinSet};

use super::envelopes;
use crate::host_services::{HostServices, ServiceError};

/// The largest number of calls of one extension that run at the same time.
/// The host refuses a call over the limit, thus an extension cannot make
/// the host start tasks without end.
const MAX_RUNNING_CALLS: usize = 64;

pub(super) struct ServiceCalls {
    services: Arc<dyn HostServices>,
    extension_id: String,
    /// The scopes that the user approved for the extension.
    scopes: Vec<String>,
    tasks: JoinSet<(u64, Result<Vec<u8>, ServiceError>)>,
    running: HashMap<u64, AbortHandle>,
}

impl ServiceCalls {
    pub(super) fn new(
        services: Arc<dyn HostServices>,
        extension_id: String,
        scopes: Vec<String>,
    ) -> Self {
        Self {
            services,
            extension_id,
            scopes,
            tasks: JoinSet::new(),
            running: HashMap::new(),
        }
    }

    /// Starts one call. If the host refuses the call, the result is the
    /// response for the extension.
    pub(super) fn start(&mut self, id: u64, call: CallRequest) -> Option<Envelope> {
        if let Err(refusal) = self.permit(id, &call.method) {
            return Some(answer(id, Err(refusal)));
        }
        let reply = self
            .services
            .call(&self.extension_id, &call.method, call.payload);
        let task = self.tasks.spawn(async move { (id, reply.await) });
        self.running.insert(id, task);
        None
    }

    fn permit(&self, id: u64, method: &str) -> Result<(), ServiceError> {
        let Some(scope) = self.services.required_scope(method) else {
            let message = format!("the host has no method `{method}`");
            return Err(ServiceError::new(ErrorCode::NotFound, message));
        };
        if !self.scopes.contains(&scope) {
            let message = format!("the extension does not have the scope `{scope}`");
            return Err(ServiceError::new(ErrorCode::PermissionDenied, message));
        }
        if self.running.contains_key(&id) {
            return Err(ServiceError::failed("a call with this `id` already runs"));
        }
        if self.running.len() >= MAX_RUNNING_CALLS {
            return Err(ServiceError::failed("too many calls run at the same time"));
        }
        Ok(())
    }

    /// Stops the work of a call that the extension cancelled. The result is
    /// the response of the call. It is `None` if the call already has its
    /// response.
    pub(super) fn cancel(&mut self, id: u64) -> Option<Envelope> {
        let task = self.running.remove(&id)?;
        task.abort();
        Some(answer(id, Err(ServiceError::cancelled())))
    }

    /// Waits for the end of one call and gives its response. Does not
    /// complete if no call runs.
    ///
    /// This function is cancel safe.
    pub(super) async fn next_response(&mut self) -> Envelope {
        loop {
            let Some(joined) = self.tasks.join_next().await else {
                return std::future::pending().await;
            };
            let (id, result) = match joined {
                Ok(done) => done,
                Err(error) => {
                    // A task that `cancel` stopped is not in `running`: its
                    // call already has its response.
                    let Some(id) = self.call_of(error.id()) else {
                        continue;
                    };
                    (id, Err(ServiceError::failed("the host service failed")))
                }
            };
            if self.running.remove(&id).is_some() {
                return answer(id, result);
            }
        }
    }

    fn call_of(&self, task: task::Id) -> Option<u64> {
        let mut calls = self.running.iter();
        calls.find_map(|(id, running)| (running.id() == task).then_some(*id))
    }

    /// Stops the work of each call. The extension stops, thus no response
    /// is necessary.
    pub(super) fn abort_all(&mut self) {
        self.tasks.abort_all();
        self.running.clear();
    }

    pub(super) fn is_empty(&self) -> bool {
        self.running.is_empty()
    }
}

/// The response of one call. A result that does not go into one frame
/// becomes an error, as the protocol says.
fn answer(id: u64, result: Result<Vec<u8>, ServiceError>) -> Envelope {
    let result = match result {
        Ok(payload) => response::Result::Call(CallResponse { payload }),
        Err(error) => response::Result::Error(error.into()),
    };
    let envelope = envelopes::response(id, result);
    let length = envelope.encoded_len();
    if length <= MAX_ENVELOPE_LENGTH {
        return envelope;
    }
    let message =
        format!("the result has {length} bytes, but the limit is {MAX_ENVELOPE_LENGTH} bytes");
    envelopes::response(
        id,
        response::Result::Error(ServiceError::failed(message).into()),
    )
}
