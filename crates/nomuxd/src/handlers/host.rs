//! The handlers of the Host service.

use std::process;

use nomux_proto::wire::v1::{HostInfoResponse, HostStopResponse, response};

use super::Context;

pub(super) fn info(context: &Context) -> response::Result {
    response::Result::HostInfo(HostInfoResponse {
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        daemon_pid: process::id(),
        session_count: u32::try_from(context.sessions.list().len()).unwrap_or(u32::MAX),
    })
}

/// The daemon stops after the client has this response.
pub(super) fn stop(context: &Context) -> response::Result {
    context.shutdown.trigger();
    response::Result::HostStop(HostStopResponse {})
}
