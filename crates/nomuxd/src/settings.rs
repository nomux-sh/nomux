//! The settings of the daemon from the global configuration file.

use std::time::Duration;

use nomux_core::configuration::{
    ConfigurationError, ConfigurationFile, RestorePolicy, default_configuration_path,
};
use tracing::warn;

/// An ended session stays for one day: the time in which a person looks for
/// the exit status and the output of a command that ended.
const DEFAULT_ENDED_SESSION_RETENTION: Duration = Duration::from_hours(24);
/// Two pooled holders: a second new session soon after the first one, for
/// example a split of a window, also takes a ready holder.
const DEFAULT_POOL_SIZE: usize = 2;
/// The pool stays while a person works, and leaves a host that nomux does
/// not use idle.
const DEFAULT_POOL_IDLE_TIME: Duration = Duration::from_mins(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settings {
    /// The restore policy of a session that has none: `default_policy` in
    /// `[restore]`.
    pub(crate) default_restore_policy: RestorePolicy,
    /// How long a session stays after its command ended: `ended_retention`
    /// in `[sessions]`.
    pub(crate) ended_session_retention: Duration,
    /// The number of holders that the daemon keeps ready for a new session:
    /// `size` in `[pool]`.
    pub(crate) pool_size: usize,
    /// How long the pool stays when no session starts: `idle_time` in
    /// `[pool]`.
    pub(crate) pool_idle_time: Duration,
    /// A pooled bash, zsh or fish starts with a start file of nomux:
    /// `shell_start_files` in `[pool]`.
    pub(crate) pool_shell_start_files: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_restore_policy: RestorePolicy::default(),
            ended_session_retention: DEFAULT_ENDED_SESSION_RETENTION,
            pool_size: DEFAULT_POOL_SIZE,
            pool_idle_time: DEFAULT_POOL_IDLE_TIME,
            pool_shell_start_files: true,
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
            pool_size: file.pool_size()?.unwrap_or(defaults.pool_size),
            pool_idle_time: file.pool_idle_time()?.unwrap_or(defaults.pool_idle_time),
            pool_shell_start_files: file
                .pool_shell_start_files()?
                .unwrap_or(defaults.pool_shell_start_files),
        })
    }
}
