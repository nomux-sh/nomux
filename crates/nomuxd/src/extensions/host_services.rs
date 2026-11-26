//! The host services: what the daemon does for an extension that calls it.
//! Each service needs a scope that the user approved. `docs/extensions.md`
//! has the list.

use std::future::ready;
use std::sync::Arc;

use nomux_ext_host::{ErrorCode, ExtensionEvent, HostServices, ServiceError, ServiceReply};
use nomux_proto::wire::v1::{self as wire, Session, SessionListResponse, event};
use prost::Message;
use tracing::debug;

use super::agent_services::{
    AGENT_CHILD, AGENT_REPORT, AGENT_REPORT_SCOPE, report_child, report_run,
};
use crate::events::EventLog;
use crate::sessions::Sessions;
use crate::status::StatusRegistry;

/// Gives the sessions of the host. The payload of the call is not read. The
/// result is a `SessionListResponse` of the control protocol.
const SESSIONS_LIST: &str = "sessions.list";
const SESSIONS_READ: &str = "sessions:read";

/// The largest payload of an event of an extension that goes to the event
/// log. An event goes to each client that subscribed, thus an extension
/// must not fill the log with a few large events.
const MAX_EVENT_PAYLOAD: usize = 64 * 1024;

pub(super) struct DaemonServices {
    sessions: Arc<Sessions>,
    status: Arc<StatusRegistry>,
    events: Arc<EventLog>,
}

impl DaemonServices {
    pub(super) fn new(
        sessions: Arc<Sessions>,
        status: Arc<StatusRegistry>,
        events: Arc<EventLog>,
    ) -> Self {
        Self {
            sessions,
            status,
            events,
        }
    }

    fn session_list(&self) -> Vec<u8> {
        let sessions = self.sessions.list(false);
        SessionListResponse {
            sessions: sessions.into_iter().map(Session::from).collect(),
        }
        .encode_to_vec()
    }
}

impl HostServices for DaemonServices {
    fn required_scope(&self, method: &str) -> Option<String> {
        let scope = match method {
            SESSIONS_LIST => SESSIONS_READ,
            AGENT_REPORT | AGENT_CHILD => AGENT_REPORT_SCOPE,
            _ => return None,
        };
        Some(scope.to_owned())
    }

    fn call(&self, _extension_id: &str, method: &str, payload: Vec<u8>) -> ServiceReply {
        let reply = match method {
            SESSIONS_LIST => Ok(self.session_list()),
            AGENT_REPORT => report_run(&self.status, &payload),
            AGENT_CHILD => report_child(&self.status, &payload),
            _ => Err(ServiceError::new(
                ErrorCode::NotFound,
                format!("the daemon has no host service `{method}`"),
            )),
        };
        Box::pin(ready(reply))
    }

    fn event(&self, extension_event: &ExtensionEvent) {
        let length = extension_event.payload.len();
        if length > MAX_EVENT_PAYLOAD {
            debug!(
                extension = %extension_event.extension_id,
                name = %extension_event.name,
                length,
                "an event of an extension is too large for the event log and is dropped"
            );
            return;
        }
        self.events
            .publish(event::Body::ExtensionEvent(wire::ExtensionEvent {
                extension_id: extension_event.extension_id.clone(),
                name: extension_event.name.clone(),
                payload: extension_event.payload.clone(),
            }));
    }
}
