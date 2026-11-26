//! The pool of holders that wait for a new session.
//!
//! A pooled holder runs the shell of the user in the home directory before
//! its session exists. A new session with the shell takes one: the session
//! then opens without the start of a process. A shell with a start file of
//! nomux changes to the directory of the session before its first prompt;
//! the pool gives any other shell only to a session in the home directory.
//! `docs/daemon.md` has the rules.
//!
//! - No pooled holder exists before the first request of a client after the
//!   start of the daemon.
//! - The pool fills one holder at a time, after a take. A start that fails
//!   stops the fill until the next new session: there is no loop of starts.
//! - The pool empties when no session started for its idle time. A new
//!   session starts it again.
//! - The daemon does not poll. One timer for the end of the idle time wakes
//!   the task of the pool.

mod shell_start;

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::mem;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::wire::SessionToken;
use tokio::process::ChildStdin;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::holder::HolderLink;

pub(crate) use self::shell_start::{ShellStart, StartFileShell, remove_pool_directory};

/// A holder in the pool, with the lease of the daemon. When the value is
/// dropped, the pipe to the holder closes, and a holder that is still
/// pooled ends.
pub(crate) struct PooledHolder {
    /// The identifier that the daemon keeps for the session of the holder.
    pub(crate) session_id: SessionId,
    pub(crate) link: HolderLink,
    /// The token that the daemon gave to the holder.
    pub(crate) token: SessionToken,
    /// The standard input of the holder. The daemon writes nothing: the
    /// value only keeps the pipe open.
    pub(crate) _pipe: ChildStdin,
}

/// What the task of the pool does next.
pub(crate) enum PoolStep {
    /// Start one pooled holder.
    Fill,
    /// Wait for a change of the pool or for the end of the idle time.
    Wait(Instant),
    /// The pool is empty and does not run. The task ends.
    Stop,
}

pub(crate) struct HolderPool {
    /// The shell of the pooled holders. `None` turns the pool off.
    shell: Option<OsString>,
    /// The shell starts with a start file of nomux, thus it can change to
    /// the directory of any session. `None`: the pool takes only sessions
    /// in the home directory.
    start_file_shell: Option<StartFileShell>,
    size: usize,
    idle_time: Duration,
    state: Mutex<PoolState>,
    changed: Notify,
}

#[derive(Default)]
struct PoolState {
    /// The oldest holder first: its shell had the most time to start.
    ready: VecDeque<PooledHolder>,
    /// The task of the pool runs.
    running: bool,
    /// The pool ran since the start of the daemon.
    started: bool,
    /// The last start of the pool or the last new session.
    last_use: Option<Instant>,
    /// A start failed. The pool fills again after the next new session.
    fill_failed: bool,
}

impl HolderPool {
    /// A pool of `size` holders with `shell`, which empties after
    /// `idle_time` without a new session. The pool is off if `size` is 0.
    /// With `start_files`, a shell that has a start file of nomux starts
    /// with it.
    pub(crate) fn new(
        shell: OsString,
        size: usize,
        idle_time: Duration,
        start_files: bool,
    ) -> Self {
        let start_file_shell = start_files.then(|| StartFileShell::of(&shell)).flatten();
        Self {
            shell: (size > 0).then_some(shell),
            start_file_shell,
            size,
            idle_time,
            state: Mutex::default(),
            changed: Notify::new(),
        }
    }

    /// The shell that a pooled holder starts.
    pub(crate) fn shell(&self) -> Option<&OsStr> {
        self.shell.as_deref()
    }

    /// The shell with a start file of nomux, if the pool starts one.
    pub(crate) fn start_file_shell(&self) -> Option<StartFileShell> {
        self.start_file_shell
    }

    /// Starts the pool for the first request of a client after the start of
    /// the daemon. Returns `true` if the caller must start the task of the
    /// pool.
    pub(crate) fn start_for_first_request(&self) -> bool {
        if self.state().started {
            return false;
        }
        self.start()
    }

    /// Starts the pool, or keeps it for the idle time from now. Returns
    /// `true` if the caller must start the task of the pool.
    pub(crate) fn start(&self) -> bool {
        if self.shell.is_none() {
            return false;
        }
        let mut state = self.state();
        state.started = true;
        state.last_use = Some(Instant::now());
        state.fill_failed = false;
        let start_task = !mem::replace(&mut state.running, true);
        drop(state);
        self.changed.notify_one();
        start_task
    }

    /// Takes a pooled holder for a new session with the shell in
    /// `working_directory`. `home` is the directory in which the pooled
    /// shells started. Returns `None` if the pool has no holder, or if the
    /// shell cannot change to the directory.
    pub(crate) fn take(&self, working_directory: &Path, home: &Path) -> Option<PooledHolder> {
        if working_directory != home && self.start_file_shell.is_none() {
            return None;
        }
        self.state().ready.pop_front()
    }

    /// The next step of the task of the pool. At the end of the idle time,
    /// the pool empties and the task stops.
    pub(crate) fn next_step(&self) -> PoolStep {
        let mut state = self.state();
        let Some(idle_end) = state.last_use.map(|last_use| last_use + self.idle_time) else {
            return PoolStep::Stop;
        };
        if !state.running {
            return PoolStep::Stop;
        }
        if Instant::now() >= idle_end {
            state.running = false;
            // The holders end when their pipes close.
            state.ready.clear();
            return PoolStep::Stop;
        }
        if !state.fill_failed && state.ready.len() < self.size {
            return PoolStep::Fill;
        }
        PoolStep::Wait(idle_end)
    }

    /// Puts a holder that started into the pool.
    pub(crate) fn add(&self, holder: PooledHolder) {
        self.state().ready.push_back(holder);
    }

    /// Stops the fill until the next new session.
    pub(crate) fn fill_failed(&self) {
        self.state().fill_failed = true;
    }

    /// Completes after the next start of the pool.
    pub(crate) async fn changed(&self) {
        self.changed.notified().await;
    }

    fn state(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
