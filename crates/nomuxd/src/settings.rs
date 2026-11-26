//! The settings of the daemon from the global configuration file.

use std::time::Duration;

use nomux_core::configuration::{
    ConfigurationError, ConfigurationFile, RestorePolicy, default_configuration_path,
};
use tracing::warn;

/// An ended session stays for one day: the time in which a person looks for
/// the exit status and the output of a command that ended.
const DEFAULT_ENDED_SESSION_RETENTION: Duration = Duration::from_hours(24);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settings {
    /// The restore policy of a session that has none: `default_policy` in
    /// `[restore]`.
    pub(crate) default_restore_policy: RestorePolicy,
    /// How long a session stays after its command ended: `ended_retention`
    /// in `[sessions]`.
    pub(crate) ended_session_retention: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_restore_policy: RestorePolicy::default(),
            ended_session_retention: DEFAULT_ENDED_SESSION_RETENTION,
        }
    }
}

impl Settings {
    /// Reads the configuration file of the user. The daemon reads it one
    /// time, at its start.
    ///
    /// A file that is not valid does not stop the daemon, because the
    /// sessions need the daemon: it writes the problem to the log and uses
    /// the default settings.
    pub(crate) fn load() -> Self {
        let Some(path) = default_configuration_path() else {
            return Self::default();
        };
        match ConfigurationFile::load(&path).and_then(|file| Self::of(&file)) {
            Ok(settings) => settings,
            Err(error) => {
                warn!(%error, "the daemon uses its default settings");
                Self::default()
            }
        }
    }

    fn of(file: &ConfigurationFile) -> Result<Self, ConfigurationError> {
        let defaults = Self::default();
        Ok(Self {
            default_restore_policy: file
                .default_restore_policy()?
                .unwrap_or(defaults.default_restore_policy),
            ended_session_retention: file
                .ended_session_retention()?
                .unwrap_or(defaults.ended_session_retention),
        })
    }
}
