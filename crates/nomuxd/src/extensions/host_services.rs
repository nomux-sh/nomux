//! The host services: what the daemon does for an extension that calls it.
//! Each service needs a scope that the user approved. `docs/extensions.md`
//! has the list.

use std::future::ready;
use std::sync::Arc;

use nomux_ext_host::{ErrorCode, HostServices, ServiceError, ServiceReply};
use nomux_proto::wire::v1::{Session, SessionListResponse};
use prost::Message;

use crate::sessions::Sessions;

/// Gives the sessions of the host. The payload of the call is not read. The
/// result is a `SessionListResponse` of the control protocol.
const SESSIONS_LIST: &str = "sessions.list";
const SESSIONS_READ: &str = "sessions:read";

pub(super) struct DaemonServices {
    sessions: Arc<Sessions>,
}

impl DaemonServices {
    pub(super) fn new(sessions: Arc<Sessions>) -> Self {
        Self { sessions }
    }

    fn session_list(&self) -> Vec<u8> {
        let sessions = self.sessions.list();
        SessionListResponse {
            sessions: sessions.into_iter().map(Session::from).collect(),
        }
        .encode_to_vec()
    }
}

impl HostServices for DaemonServices {
    fn required_scope(&self, method: &str) -> Option<String> {
        (method == SESSIONS_LIST).then(|| SESSIONS_READ.to_owned())
    }

    fn call(&self, _extension_id: &str, method: &str, _payload: Vec<u8>) -> ServiceReply {
        let reply = match method {
            SESSIONS_LIST => Ok(self.session_list()),
            _ => Err(ServiceError::new(
                ErrorCode::NotFound,
                format!("the daemon has no host service `{method}`"),
            )),
        };
        Box::pin(ready(reply))
    }
}
