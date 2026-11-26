//! The trash place: `Worktree.Trash`, `Worktree.Restore`,
//! `Worktree.EmptyTrash`, and the removal at the end of the grace time.

use std::collections::HashSet;
use std::path::Path;

use nomux_proto::wire::v1::{
    Error, WorktreeChange, WorktreeChanged, WorktreeDeleteRequest, WorktreeEmptyTrashRequest,
    WorktreeEmptyTrashResponse, WorktreeMoveFromTrashRequest, WorktreeRestoreRequest,
    WorktreeRestoreResponse, WorktreeTrashRemoval, WorktreeTrashRequest, WorktreeTrashResponse,
};
use tracing::warn;

use super::store::TrashEntry;
use super::{Sources, Worktrees, failed, git_changes, not_found};
use crate::clock::now;

impl Worktrees {
    /// `Worktree.Trash`.
    pub(crate) fn trash(
        &self,
        request: &WorktreeTrashRequest,
        sources: &Sources<'_>,
    ) -> Result<WorktreeTrashResponse, Error> {
        let projects: HashSet<String> = sources
            .workspace
            .projects(&request.project_id)?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let mut entries: Vec<TrashEntry> = self
            .store
            .trash_entries()
            .into_iter()
            .filter(|entry| request.project_id.is_empty() || projects.contains(&entry.project_id))
            .collect();
        entries.sort_by_key(|entry| entry.trashed_at_ms);
        Ok(WorktreeTrashResponse {
            entries: entries.iter().map(TrashEntry::wire).collect(),
        })
    }

    /// `Worktree.Restore`.
    pub(crate) async fn restore(
        &self,
        request: &WorktreeRestoreRequest,
        sources: &Sources<'_>,
    ) -> Result<WorktreeRestoreResponse, Error> {
        let entry = self.entry_named(&request.name)?;
        if entry.path.symlink_metadata().is_ok() {
            return Err(failed(format!(
                "{} exists: the worktree cannot move back there",
                entry.path.display()
            )));
        }
        let _cleaning = self.cleaning.lock().await;
        git_changes::move_from_trash(
            sources.extensions,
            WorktreeMoveFromTrashRequest {
                repository: entry.repository.display().to_string(),
                trash_path: entry.trash_path.display().to_string(),
                path: entry.path.display().to_string(),
            },
        )
        .await
        .map_err(failed)?;
        self.store
            .remove_trash_entry(&entry.id)
            .map_err(|error| failed(format!("the worktree file was not saved: {error}")))?;
        self.trash_changed.notify_one();
        self.publish(changed(&entry, WorktreeChange::Restored, false));
        Ok(WorktreeRestoreResponse {
            entry: Some(entry.wire()),
        })
    }

    /// `Worktree.EmptyTrash`.
    pub(crate) async fn empty_trash(
        &self,
        request: &WorktreeEmptyTrashRequest,
        sources: &Sources<'_>,
    ) -> Result<WorktreeEmptyTrashResponse, Error> {
        let entries = self.store.trash_entries();
        for id in &request.ids {
            if !entries.iter().any(|entry| &entry.id == id) {
                return Err(not_found(format!("no trash entry has the identifier {id}")));
            }
        }
        let _cleaning = self.cleaning.lock().await;
        let mut removals = Vec::new();
        for entry in entries {
            if request.ids.is_empty() || request.ids.contains(&entry.id) {
                removals.push(self.remove(entry, sources).await);
            }
        }
        Ok(WorktreeEmptyTrashResponse { removals })
    }

    /// Removes the entries whose grace time ended. An entry that the git
    /// extension refuses stays and is not tried again until the daemon
    /// starts again: `failed` has it. The caller holds `cleaning`.
    pub(super) async fn remove_expired(&self, sources: &Sources<'_>, failed: &mut HashSet<String>) {
        let now_ms = now().0;
        for entry in self.store.trash_entries() {
            if entry.expires_at_ms > now_ms || failed.contains(&entry.id) {
                continue;
            }
            let id = entry.id.clone();
            let removal = self.remove(entry, sources).await;
            if !removal.removed {
                warn!(id, message = removal.message, "a trash entry stays");
                failed.insert(id);
            }
        }
    }

    /// The end of the grace time of the next entry that can be removed:
    /// milliseconds after the Unix epoch.
    pub(super) fn next_expiry(&self, failed: &HashSet<String>) -> Option<u64> {
        self.store
            .trash_entries()
            .iter()
            .filter(|entry| !failed.contains(&entry.id))
            .map(|entry| entry.expires_at_ms)
            .min()
    }

    async fn remove(&self, entry: TrashEntry, sources: &Sources<'_>) -> WorktreeTrashRemoval {
        let response = git_changes::delete(
            sources.extensions,
            WorktreeDeleteRequest {
                repository: entry.repository.display().to_string(),
                path: entry.trash_path.display().to_string(),
                expected_head: entry.head.clone(),
                allowed_ignored_paths: entry.allowed_ignored_paths.clone(),
                allow_unpushed: entry.allow_unpushed,
                delete_branch: entry.deletes_branch,
                base_branch: self.settings.base.clone(),
            },
        )
        .await;
        let mut removal = WorktreeTrashRemoval {
            entry: Some(entry.wire()),
            ..WorktreeTrashRemoval::default()
        };
        match response {
            Ok(response) if response.refusals.is_empty() => {
                if let Err(error) = self.store.remove_trash_entry(&entry.id) {
                    warn!(%error, "the worktree file was not saved");
                }
                self.trash_changed.notify_one();
                self.publish(changed(
                    &entry,
                    WorktreeChange::Removed,
                    response.branch_deleted,
                ));
                removal.removed = true;
                removal.branch_deleted = response.branch_deleted;
                removal.branch_kept_reason = response.branch_kept_reason;
            }
            Ok(response) => removal.message = response.message,
            Err(message) => removal.message = message,
        }
        removal
    }

    /// The entry with the identifier `name`, the path `name`, or the one
    /// entry whose path ends with the part `name`.
    fn entry_named(&self, name: &str) -> Result<TrashEntry, Error> {
        let entries = self.store.trash_entries();
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.id == name || entry.path == Path::new(name))
        {
            return Ok(entry.clone());
        }
        let mut by_name = entries
            .iter()
            .filter(|entry| entry.path.file_name().is_some_and(|part| part == name));
        match (by_name.next(), by_name.next()) {
            (Some(entry), None) => Ok(entry.clone()),
            (Some(_), Some(_)) => Err(not_found(format!(
                "more than one trash entry has the name {name}: give its identifier"
            ))),
            (None, _) => Err(not_found(format!("no trash entry has the name {name}"))),
        }
    }
}

fn changed(entry: &TrashEntry, change: WorktreeChange, branch_deleted: bool) -> WorktreeChanged {
    WorktreeChanged {
        project_id: entry.project_id.clone(),
        path: entry.path.display().to_string(),
        change: change.into(),
        bytes: 0,
        automatic: false,
        branch: entry.branch.clone(),
        branch_deleted,
    }
}
