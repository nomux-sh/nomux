//! A host without a sandbox interface that nomux uses: it applies no
//! feature.

use std::process::Command;

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxPlan};

use crate::{HostSupport, SandboxError};

/// The limits of a plan on this host: none. [`prepare`] gives one only for a
/// plan that requires no feature.
#[derive(Debug)]
pub struct Prepared {
    enforced: FeatureSet,
}

impl Prepared {
    /// The features that apply: none.
    #[must_use]
    pub fn enforced(&self) -> FeatureSet {
        self.enforced
    }

    /// Changes nothing: the host applies no limit.
    pub fn install(self, _command: &mut Command) {}

    /// Changes nothing: the host applies no limit.
    ///
    /// # Errors
    ///
    /// Never.
    pub fn apply_to_this_process(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn probe() -> HostSupport {
    HostSupport {
        landlock_abi: 0,
        seccomp: false,
        system: std::env::consts::OS.to_owned(),
    }
}

pub(crate) fn features(_support: &HostSupport) -> FeatureSet {
    FeatureSet::EMPTY
}

pub(crate) fn detail(support: &HostSupport) -> String {
    format!(
        "{}: nomux has no sandbox for this operating system yet",
        support.system
    )
}

pub(crate) fn prepare(plan: &SandboxPlan, support: &HostSupport) -> Result<Prepared, SandboxError> {
    if !plan.require.is_empty() {
        return Err(SandboxError::Unenforceable {
            profile: plan.profile.clone(),
            missing: plan.require,
            detail: detail(support),
        });
    }
    Ok(Prepared {
        enforced: FeatureSet::EMPTY,
    })
}
