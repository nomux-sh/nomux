//! The restore policy of a session.

use std::fmt;
use std::str::FromStr;

/// When the daemon starts the command of a dormant session again. A session
/// is dormant when its holder is gone and its command did not end: for
/// example, after a restart of the host.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RestorePolicy {
    /// When the daemon starts.
    Auto,
    /// When a client attaches to the session for the first time.
    #[default]
    OnFocus,
    /// Only when a client asks for the start.
    Manual,
}

impl RestorePolicy {
    /// The name of the policy in the configuration file, on the command line
    /// and in the state of a session.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OnFocus => "on-focus",
            Self::Manual => "manual",
        }
    }
}

impl fmt::Display for RestorePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The text is not the name of a restore policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownRestorePolicy;

impl fmt::Display for UnknownRestorePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a restore policy is `auto`, `on-focus` or `manual`")
    }
}

impl std::error::Error for UnknownRestorePolicy {}

impl FromStr for RestorePolicy {
    type Err = UnknownRestorePolicy;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        [Self::Auto, Self::OnFocus, Self::Manual]
            .into_iter()
            .find(|policy| policy.as_str() == text)
            .ok_or(UnknownRestorePolicy)
    }
}
