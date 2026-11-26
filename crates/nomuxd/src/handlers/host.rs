//! The handlers of the Host service.

use std::process;

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxFeature};
use nomux_proto::wire::v1::{
    HostInfoResponse, HostStopResponse, MemoryLimitSupport, SandboxSupport, response,
};

use super::Context;

pub(super) fn info(context: &Context) -> response::Result {
    response::Result::HostInfo(HostInfoResponse {
        daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
        daemon_pid: process::id(),
        session_count: u32::try_from(context.sessions.list(false).len()).unwrap_or(u32::MAX),
        memory_limit: Some(memory_limit_support(context)),
        sandbox: Some(sandbox_support(context)),
    })
}

fn sandbox_support(context: &Context) -> SandboxSupport {
    let support = context.sessions.sandbox_support();
    let enforced = support.features();
    let names = |features: FeatureSet| {
        features
            .names()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    SandboxSupport {
        enforced: names(enforced),
        missing: names(FeatureSet::from_iter(SandboxFeature::ALL).without(enforced)),
        detail: support.detail(),
    }
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
