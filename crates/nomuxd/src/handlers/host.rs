//! The handlers of the Host service.

use std::process;

use nomux_proto::wire::v1::{HostInfoResponse, HostStopResponse, MemoryLimitSupport, response};

use super::Context;

pub(super) fn info(context: &Context) -> response::Result {
    response::Result::HostInfo(HostInfoResponse {
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        daemon_pid: process::id(),
        session_count: u32::try_from(context.sessions.list(false).len()).unwrap_or(u32::MAX),
        memory_limit: Some(memory_limit_support(context)),
    })
}

fn memory_limit_support(context: &Context) -> MemoryLimitSupport {
    let (applied, detail) = context.sessions.memory_limit_support();
    MemoryLimitSupport { applied, detail }
}

/// The daemon stops after the client has this response.
pub(super) fn stop(context: &Context) -> response::Result {
    context.shutdown.trigger();
    response::Result::HostStop(HostStopResponse {})
}
