//! The settings of the daemon from the global configuration file.

use std::time::Duration;

use nomux_core::configuration::{
    AutoClean, BuildOutputRule, ConfigurationError, ConfigurationFile, RestorePolicy, StopSignal,
    default_configuration_path,
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

/// From ADR-0009: a host with a few open agents keeps them all, and a host
/// with many idle agents gives the memory of the oldest ones back.
const DEFAULT_MAX_LIVE_AGENTS: usize = 12;

/// From ADR-0009: an agent that did not change for a minute is not in use.
const DEFAULT_HIBERNATE_AFTER: Duration = Duration::from_secs(60);

/// The signal of the end of a terminal first, which an agent takes as the
/// end of its session, then the request to end, and at last the end that a
/// program cannot refuse.
const DEFAULT_STOP_SIGNALS: [StopSignal; 3] =
    [StopSignal::Hangup, StopSignal::Terminate, StopSignal::Kill];

/// An agent that writes its conversation to the disk needs a moment.
const DEFAULT_STOP_WAIT: Duration = Duration::from_secs(5);

/// From the design of issue #37: two weeks without a commit or a checkout.
const DEFAULT_STALE_AFTER: Duration = Duration::from_hours(14 * 24);
/// A cold build of a small project takes seconds, thus a day of grace is
/// cheap.
const DEFAULT_GRACE: Duration = Duration::from_hours(24);
/// A size walk of a large `node_modules` takes seconds: at most one an hour.
const DEFAULT_SIZE_TTL: Duration = Duration::from_hours(1);
const DEFAULT_WARN_FREE: u64 = 10 << 30;
const DEFAULT_CRITICAL_FREE: u64 = 3 << 30;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// The memory limit of one session in bytes: `memory_limit` in
    /// `[sessions]`. `None`: no limit.
    pub(crate) session_memory_limit: Option<u64>,
    /// The memory limit of all sessions together in bytes:
    /// `total_memory_limit` in `[sessions]`. `None`: no limit.
    pub(crate) total_memory_limit: Option<u64>,
    /// When the daemon hibernates idle agent sessions.
    pub(crate) hibernation: HibernationSettings,
    /// The rules of the worktrees: the `[worktrees]` table.
    pub(crate) worktrees: WorktreeSettings,
}

/// The settings of the worktrees: the `[worktrees]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeSettings {
    /// The cleans that the daemon runs without a command: `auto`. Empty:
    /// the daemon only reports.
    pub(crate) auto: Vec<AutoClean>,
    /// A worktree without activity for this time is stale: `stale_after`.
    pub(crate) stale_after: Duration,
    /// How long a worktree stays in the trash place: `grace`.
    pub(crate) grace: Duration,
    /// A size is old after this time: `size_ttl`.
    pub(crate) size_ttl: Duration,
    /// The base branch of the merge rules: `base`. Empty: the rule of the
    /// git extension.
    pub(crate) base: String,
    /// The free bytes below which the daemon warns: `warn_free` in
    /// `[worktrees.disk]`.
    pub(crate) warn_free: u64,
    /// The free bytes below which the free space is critical:
    /// `critical_free` in `[worktrees.disk]`.
    pub(crate) critical_free: u64,
    /// The build-output rules of the user: `extra` in
    /// `[worktrees.build_output]`.
    pub(crate) build_output_rules: Vec<BuildOutputRule>,
}

impl Default for WorktreeSettings {
    fn default() -> Self {
        Self {
            auto: Vec::new(),
            stale_after: DEFAULT_STALE_AFTER,
            grace: DEFAULT_GRACE,
            size_ttl: DEFAULT_SIZE_TTL,
            base: String::new(),
            warn_free: DEFAULT_WARN_FREE,
            critical_free: DEFAULT_CRITICAL_FREE,
            build_output_rules: Vec::new(),
        }
    }
}

/// The settings of the hibernation: the `[agents]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HibernationSettings {
    /// The largest number of live agent sessions: `max_live`. 0 turns the
    /// count trigger off.
    pub(crate) max_live: usize,
    /// How long an agent session is idle before it can hibernate:
    /// `hibernate_after`.
    pub(crate) hibernate_after: Duration,
    /// If the memory pressure of the host hibernates idle agent sessions:
    /// `hibernate_on_pressure`.
    pub(crate) on_pressure: bool,
    /// The signals that end the command, in their sequence:
    /// `stop_signals`. The last one is `KILL`.
    pub(crate) stop_signals: Vec<StopSignal>,
    /// The time after each signal: `stop_wait`.
    pub(crate) stop_wait: Duration,
}

impl Default for HibernationSettings {
    fn default() -> Self {
        Self {
            max_live: DEFAULT_MAX_LIVE_AGENTS,
            hibernate_after: DEFAULT_HIBERNATE_AFTER,
            on_pressure: true,
            stop_signals: DEFAULT_STOP_SIGNALS.to_vec(),
            stop_wait: DEFAULT_STOP_WAIT,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_restore_policy: RestorePolicy::default(),
            ended_session_retention: DEFAULT_ENDED_SESSION_RETENTION,
            pool_size: DEFAULT_POOL_SIZE,
            pool_idle_time: DEFAULT_POOL_IDLE_TIME,
            pool_shell_start_files: true,
            session_memory_limit: None,
            total_memory_limit: None,
            hibernation: HibernationSettings::default(),
            worktrees: WorktreeSettings::default(),
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
            session_memory_limit: file.session_memory_limit()?,
            total_memory_limit: file.total_memory_limit()?,
            hibernation: HibernationSettings {
                max_live: file
                    .max_live_agents()?
                    .unwrap_or(defaults.hibernation.max_live),
                hibernate_after: file
                    .hibernate_after()?
                    .unwrap_or(defaults.hibernation.hibernate_after),
                on_pressure: file
                    .hibernate_on_pressure()?
                    .unwrap_or(defaults.hibernation.on_pressure),
                stop_signals: file
                    .stop_signals()?
                    .unwrap_or(defaults.hibernation.stop_signals),
                stop_wait: file.stop_wait()?.unwrap_or(defaults.hibernation.stop_wait),
            },
            worktrees: WorktreeSettings {
                auto: file.worktree_auto_cleans()?.unwrap_or_default(),
                stale_after: file
                    .worktree_stale_after()?
                    .unwrap_or(defaults.worktrees.stale_after),
                grace: file.worktree_grace()?.unwrap_or(defaults.worktrees.grace),
                size_ttl: file
                    .worktree_size_ttl()?
                    .unwrap_or(defaults.worktrees.size_ttl),
                base: file.worktree_base()?.unwrap_or_default(),
                warn_free: file
                    .worktree_warn_free()?
                    .unwrap_or(defaults.worktrees.warn_free),
                critical_free: file
                    .worktree_critical_free()?
                    .unwrap_or(defaults.worktrees.critical_free),
                build_output_rules: file.worktree_build_output_rules()?.unwrap_or_default(),
            },
        })
    }
}
