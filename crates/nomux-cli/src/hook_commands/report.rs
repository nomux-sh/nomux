//! The result of a hook command that did not fail.

use std::path::PathBuf;

use nomux_client::ToolHooks;

use crate::report::Report;

#[derive(Debug)]
pub(crate) enum HooksReport {
    /// `hook`: what the hook writes to its standard output for the tool.
    Answered { output: Vec<u8> },
    /// `hooks install` or `hooks uninstall`.
    Changed {
        action: HooksAction,
        tool: String,
        path: PathBuf,
        /// The unified diff. Empty if nothing changes.
        diff: String,
        /// `false` for a dry run.
        applied: bool,
        /// For an uninstall: `true` if the file gets its bytes from before
        /// the install, `false` if only the entries of nomux go away.
        restores_original: bool,
    },
    /// `hooks status`.
    Statuses(Vec<HookToolStatus>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HooksAction {
    Install,
    Uninstall,
}

/// The hooks of one tool for `hooks status`.
#[derive(Debug)]
pub(crate) struct HookToolStatus {
    pub(crate) tool: String,
    /// The identifier of the adapter extension. `None` if the tool has no
    /// adapter.
    pub(crate) adapter: Option<String>,
    /// The hooks in the settings file, or why they are not known.
    pub(crate) hooks: Result<ToolHooks, String>,
}

impl From<HooksReport> for Report {
    fn from(report: HooksReport) -> Self {
        Self::Hooks(report)
    }
}
