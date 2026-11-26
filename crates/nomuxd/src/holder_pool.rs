//! The pool of holders that wait for a new session.
//!
//! A pooled holder runs the shell of the user in the home directory before
//! its session exists. A new session with the shell takes one: the session
//! then opens without the start of a process. `docs/daemon.md` has the
//! rules.
//!
//! - No pooled holder exists before the first request of a client after the
//!   start of the daemon.
//! - The pool fills one holder at a time, after a take. A start that fails
//!   stops the fill until the next new session: there is no loop of starts.
//! - The pool empties when no session started for its idle time. A new
//!   session starts it again.
//! - The daemon does not poll. One timer for the end of the idle time wakes
//!   the task of the pool.

mod directory_change;

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::mem;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use tokio::process::ChildStdin;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::holder::HolderLink;

/// A holder in the pool, with the lease of the daemon. When the value is
/// dropped, the pipe to the holder closes, and a holder that is still
/// pooled ends.
pub(crate) struct PooledHolder {
    /// The identifier that the daemon keeps for the session of the holder.
    pub(crate) session_id: SessionId,
    pub(crate) link: HolderLink,
    /// The standard input of the holder. The daemon writes nothing: the
    /// value only keeps the pipe open.
    pub(crate) _pipe: ChildStdin,
}

/// A pooled holder that a new session takes, and the input that changes the
/// directory of its shell.
pub(crate) struct TakenHolder {
    pub(crate) holder: PooledHolder,
    pub(crate) directory_change: Option<Vec<u8>>,
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
    /// `idle_time` without a new session. The pool is off if `size` is 0,
    /// or if the daemon cannot change the directory of `shell`.
    pub(crate) fn new(shell: OsString, size: usize, idle_time: Duration) -> Self {
        let shell = (size > 0 && directory_change::is_known_shell(&shell)).then_some(shell);
        Self {
            shell,
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
    pub(crate) fn take(&self, working_directory: &Path, home: &Path) -> Option<TakenHolder> {
        let directory_change = if working_directory == home {
            None
        } else {
            Some(directory_change::input(working_directory)?)
        };
        let holder = self.state().ready.pop_front()?;
        Some(TakenHolder {
            holder,
            directory_change,
        })
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
