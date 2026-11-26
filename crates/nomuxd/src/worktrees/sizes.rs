//! The sizes of the worktrees in memory, and the walks that measure them.
//!
//! The daemon measures only when a client asks for the list and a size is
//! absent or old. It has no timer: a daemon that no client asks measures
//! nothing. One walk runs at a time, on a thread that exists only during the
//! walk.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nomux_proto::wire::v1::{BuildOutputSize, WorktreeSize};
use tokio::task::{spawn_blocking, spawn_local};
use tracing::debug;

use super::size_walk::{Measured, WalkTarget, walk_project};
use crate::clock::now;

/// A size is old after this time.
pub(super) const SIZE_LIFETIME: Duration = Duration::from_hours(1);

/// The worktrees of one Project to measure in one walk.
pub(super) struct ProjectWalk {
    /// The directory of the Project.
    pub(super) project: PathBuf,
    pub(super) targets: Vec<WalkTarget>,
    /// The sessions in each target when the walk starts, in the sequence of
    /// `targets`. The end of one of them makes the size old.
    pub(super) sessions: Vec<Vec<String>>,
}

#[derive(Default)]
pub(super) struct SizeCache {
    entries: Mutex<HashMap<PathBuf, Cached>>,
    /// The Projects whose walk waits or runs in the background.
    pending: Mutex<HashSet<PathBuf>>,
    /// One walk at a time.
    walking: tokio::sync::Mutex<()>,
}

struct Cached {
    project: PathBuf,
    measured: Measured,
    measured_at_ms: u64,
    sessions: Vec<String>,
}

impl SizeCache {
    /// The size of the worktree `root`, and `true` if the size is absent or
    /// old. `live_sessions` are the sessions that are not ended.
    pub(super) fn size_of(
        &self,
        root: &Path,
        live_sessions: &HashSet<String>,
    ) -> (Option<WorktreeSize>, bool) {
        let entries = self.entries();
        let Some(cached) = entries.get(root) else {
            return (None, true);
        };
        let lifetime = u64::try_from(SIZE_LIFETIME.as_millis()).unwrap_or(u64::MAX);
        let is_old = now().0.saturating_sub(cached.measured_at_ms) > lifetime
            || cached
                .sessions
                .iter()
                .any(|session| !live_sessions.contains(session));
        (Some(wire_size(cached, is_old)), is_old)
    }

    /// Forgets the size of each worktree of `project` that is not in
    /// `roots`.
    pub(super) fn keep_only(&self, project: &Path, roots: &HashSet<&Path>) {
        self.entries()
            .retain(|root, cached| cached.project != project || roots.contains(root.as_path()));
    }

    /// Measures the worktrees of `walk` and keeps the result.
    pub(super) async fn measure(&self, walk: ProjectWalk) {
        let _walking = self.walking.lock().await;
        let ProjectWalk {
            project,
            targets,
            sessions,
        } = walk;
        let roots: Vec<PathBuf> = targets.iter().map(|target| target.root.clone()).collect();
        let measured = match spawn_blocking(move || walk_project(&targets)).await {
            Ok(measured) => measured,
            Err(error) => {
                debug!(project = %project.display(), %error, "the size walk did not end");
                return;
            }
        };
        let measured_at_ms = now().0;
        let mut entries = self.entries();
        for ((root, measured), sessions) in roots.into_iter().zip(measured).zip(sessions) {
            entries.insert(
                root,
                Cached {
                    project: project.clone(),
                    measured,
                    measured_at_ms,
                    sessions,
                },
            );
        }
    }

    /// Measures the worktrees of `walk` after the response, unless a walk of
    /// the same Project waits or runs.
    pub(super) fn measure_later(self: &Arc<Self>, walk: ProjectWalk) {
        if !self.pending().insert(walk.project.clone()) {
            return;
        }
        let cache = Arc::clone(self);
        spawn_local(async move {
            let project = walk.project.clone();
            cache.measure(walk).await;
            cache.pending().remove(&project);
        });
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<PathBuf, Cached>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn pending(&self) -> MutexGuard<'_, HashSet<PathBuf>> {
        // No code panics while it holds the lock, thus the set is whole.
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn wire_size(cached: &Cached, is_old: bool) -> WorktreeSize {
    let mut build_outputs: Vec<BuildOutputSize> = cached
        .measured
        .build_outputs
        .iter()
        .map(|(path, rule, bytes)| BuildOutputSize {
            path: path.clone(),
            rule: (*rule).to_owned(),
            bytes: *bytes,
        })
        .collect();
    build_outputs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    WorktreeSize {
        total_bytes: cached.measured.total_bytes,
        build_output_bytes: build_outputs.iter().map(|output| output.bytes).sum(),
        measured_at_ms: cached.measured_at_ms,
        is_old,
        build_outputs,
    }
}
