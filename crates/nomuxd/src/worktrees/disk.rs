//! The levels of the free space of the file systems of the Projects. The
//! daemon sends `WorktreeDiskPressure` when the level of a file system
//! changes. The event proposes the dry run of a clean and deletes nothing.

use std::path::PathBuf;

use nomux_proto::wire::v1::{WorktreeDiskLevel, WorktreeDiskPressure, event};

use super::{Worktrees, host_facts, lock};

/// The command that the event proposes: it shows the plan and changes
/// nothing.
const DRY_RUN: &str = "nomux worktree clean --all --dry-run";

/// Reads the free space of the file system of each Project and sends an
/// event for each level that changed.
pub(super) fn check(worktrees: &Worktrees, projects: &[(String, PathBuf)]) {
    let mut changed_to_pressure = false;
    for (_, directory) in projects {
        let Some(device) = host_facts::device(directory) else {
            continue;
        };
        let (free_bytes, _) = host_facts::space(directory);
        let (level, threshold_bytes) = level(worktrees, free_bytes);
        let previous = lock(&worktrees.disk_levels)
            .insert(device, level.into())
            .unwrap_or(WorktreeDiskLevel::Unspecified.into());
        if previous == i32::from(level) {
            continue;
        }
        changed_to_pressure |= level != WorktreeDiskLevel::Unspecified;
        let (reclaimable_bytes, build_output_bytes) = lock(&worktrees.reclaimable)
            .get(&device)
            .copied()
            .unwrap_or_default();
        worktrees
            .events
            .publish(event::Body::WorktreeDiskPressure(WorktreeDiskPressure {
                path: directory.display().to_string(),
                level: level.into(),
                free_bytes,
                threshold_bytes,
                reclaimable_bytes,
                build_output_bytes,
                command: DRY_RUN.to_owned(),
            }));
    }
    if changed_to_pressure && !worktrees.settings.auto.is_empty() {
        worktrees.auto_wanted.notify_one();
    }
}

fn level(worktrees: &Worktrees, free_bytes: u64) -> (WorktreeDiskLevel, u64) {
    let settings = &worktrees.settings;
    if free_bytes < settings.critical_free {
        (WorktreeDiskLevel::Critical, settings.critical_free)
    } else if free_bytes < settings.warn_free {
        (WorktreeDiskLevel::Warning, settings.warn_free)
    } else {
        (WorktreeDiskLevel::Unspecified, 0)
    }
}
