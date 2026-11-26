//! The run of a plan: `Worktree.Clean`, and the cleans of the rules of the
//! settings. Before each item the daemon makes the plan again: an item runs
//! only when the new plan has it without a refusal.

use std::collections::HashMap;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use nomux_proto::wire::v1::{
    Error, WorktreeChange, WorktreeChanged, WorktreeCleanOutcome, WorktreeCleanRequest,
    WorktreeCleanResponse, WorktreeCleanResult, WorktreeDeleteRequest, WorktreeMoveToTrashRequest,
    WorktreePlanItem, WorktreePlanStep, WorktreePruneRequest,
};
use tokio::task::spawn_blocking;
use tracing::warn;

use super::plan::{self, Selection, StoredPlan, random_id};
use super::store::TrashEntry;
use super::{
    Described, Sizes, Sources, Worktrees, deletion, disk, failed, git_changes, host_facts,
    last_part, not_found,
};
use crate::clock::now;

const OWNER_ONLY: u32 = 0o700;

impl Worktrees {
    /// `Worktree.Clean`.
    pub(crate) async fn clean(
        &self,
        request: &WorktreeCleanRequest,
        sources: Sources<'_>,
    ) -> Result<WorktreeCleanResponse, Error> {
        let plan = self.plans.get(&request.plan_id).ok_or_else(|| {
            not_found(format!(
                "no plan has the identifier {}: a plan stays 15 minutes, and a daemon that \
                 starts again has none. Make the plan again",
                request.plan_id
            ))
        })?;
        let Ok(_cleaning) = self.cleaning.try_lock() else {
            return Err(failed(
                "a different clean runs: try again after it".to_owned(),
            ));
        };
        Ok(self.run(plan, &sources).await)
    }

    /// Runs the items of `plan`. The caller holds `cleaning`.
    pub(super) async fn run(
        &self,
        plan: StoredPlan,
        sources: &Sources<'_>,
    ) -> WorktreeCleanResponse {
        self.finish_deletions().await;
        let before = free_space(&plan.items);
        let mut run = Run {
            worktrees: self,
            sources,
            selection: &plan.selection,
            results: Vec::new(),
            failed_worktrees: Vec::new(),
        };
        // The prune steps of a Project are at the end of its items: one call
        // prunes them together.
        let mut prunes: Vec<WorktreePlanItem> = Vec::new();
        for item in plan.items {
            if prunes
                .first()
                .is_some_and(|prune| prune.project_id != item.project_id)
            {
                run.prune(std::mem::take(&mut prunes)).await;
            }
            match item.step() {
                WorktreePlanStep::Prune | WorktreePlanStep::Repair => prunes.push(item),
                _ => run.item(item).await,
            }
        }
        if !prunes.is_empty() {
            run.prune(prunes).await;
        }
        let results = run.results;
        let freed_bytes = before
            .values()
            .map(|(directory, free)| {
                host_facts::flush(directory);
                host_facts::space(directory).0.saturating_sub(*free)
            })
            .sum();
        if let Ok(projects) = sources.workspace.projects("") {
            disk::check(self, &projects);
        }
        WorktreeCleanResponse {
            planned_bytes: results
                .iter()
                .filter(|result| result.outcome() == WorktreeCleanOutcome::Done)
                .filter_map(|result| result.item.as_ref())
                .map(|item| item.bytes)
                .sum(),
            freed_bytes,
            results,
        }
    }

    /// Deletes the build output directories whose deletion a stop of the
    /// daemon interrupted.
    async fn finish_deletions(&self) {
        for directory in self.store.deleting() {
            let deleting = directory.clone();
            match spawn_blocking(move || deletion::delete(&deleting)).await {
                Ok(Ok(())) => {
                    if let Err(error) = self.store.remove_deleting(&directory) {
                        warn!(%error, "the worktree file was not saved");
                    }
                }
                Ok(Err(error)) => {
                    warn!(directory = %directory.display(), %error, "a deletion did not end");
                }
                Err(error) => warn!(%error, "a deletion did not end"),
            }
        }
    }
}

/// The state of one run of a plan.
struct Run<'a, 'b> {
    worktrees: &'a Worktrees,
    sources: &'a Sources<'b>,
    selection: &'a Selection,
    results: Vec<WorktreeCleanResult>,
    /// The worktrees with a step that failed: their later steps do not run.
    failed_worktrees: Vec<String>,
}

impl Run<'_, '_> {
    async fn item(&mut self, item: WorktreePlanItem) {
        if !item.refusals.is_empty() {
            return self.result(item, WorktreeCleanOutcome::Refused, String::new());
        }
        if self.failed_worktrees.contains(&item.worktree_path) {
            let message = "an earlier step of the worktree failed".to_owned();
            return self.result(item, WorktreeCleanOutcome::Failed, message);
        }
        let Some((project, fresh)) = self.fresh(&item).await else {
            let message = "the facts of the worktree changed after the plan".to_owned();
            return self.result(item, WorktreeCleanOutcome::Changed, message);
        };
        if !fresh.refusals.is_empty() {
            let item = WorktreePlanItem {
                refusals: fresh.refusals,
                ..item
            };
            return self.result(item, WorktreeCleanOutcome::Refused, String::new());
        }
        let done = match item.step() {
            WorktreePlanStep::BuildOutput => self.delete_build_output(&item).await,
            WorktreePlanStep::Trash => self.move_to_trash(&item, &project).await,
            WorktreePlanStep::Delete => self.delete(&item, &project).await,
            _ => Err("the step is not a step of a worktree".to_owned()),
        };
        match done {
            Ok(Outcome::Done) => self.result(item, WorktreeCleanOutcome::Done, String::new()),
            Ok(Outcome::Refused(refusals, message)) => {
                let item = WorktreePlanItem { refusals, ..item };
                self.result(item, WorktreeCleanOutcome::Refused, message);
            }
            Err(message) => {
                self.failed_worktrees.push(item.worktree_path.clone());
                self.result(item, WorktreeCleanOutcome::Failed, message);
            }
        }
    }

    /// The same item in a new plan of its Project, and that Project.
    async fn fresh(&self, item: &WorktreePlanItem) -> Option<(Described, WorktreePlanItem)> {
        let projects = self
            .worktrees
            .snapshot(&item.project_id, self.sources, Sizes::Cached)
            .await
            .ok()?;
        let key = plan::key(item);
        let fresh = plan::items(&projects, self.selection)
            .into_iter()
            .find(|fresh| plan::key(fresh) == key)?;
        let project = projects.into_iter().next()?;
        Some((project, fresh))
    }

    async fn delete_build_output(&self, item: &WorktreePlanItem) -> Result<Outcome, String> {
        let directory = PathBuf::from(&item.path);
        let renamed = deletion::deleting_name(&directory, &random_id());
        let store = &self.worktrees.store;
        store
            .add_deleting(&renamed)
            .map_err(|error| format!("the worktree file was not saved: {error}"))?;
        if let Err(error) = std::fs::rename(&directory, &renamed) {
            store.remove_deleting(&renamed).ok();
            return Err(format!("{} was not renamed: {error}", directory.display()));
        }
        let deleting = renamed.clone();
        let deleted = spawn_blocking(move || deletion::delete(&deleting))
            .await
            .map_err(|error| error.to_string())
            .and_then(|deleted| deleted.map_err(|error| error.to_string()));
        self.worktrees.sizes.forget(Path::new(&item.worktree_path));
        deleted.map_err(|error| format!("{} was not deleted: {error}", renamed.display()))?;
        store
            .remove_deleting(&renamed)
            .map_err(|error| format!("the worktree file was not saved: {error}"))?;
        self.publish(item, WorktreeChange::BuildOutputRemoved, false);
        Ok(Outcome::Done)
    }

    async fn move_to_trash(
        &self,
        item: &WorktreePlanItem,
        project: &Described,
    ) -> Result<Outcome, String> {
        let trash_path = PathBuf::from(&item.path);
        if let Some(place) = trash_path.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(OWNER_ONLY)
                .create(place)
                .map_err(|error| format!("{} was not made: {error}", place.display()))?;
        }
        let allowed = self.allowed_ignored_paths(item, project);
        let response = git_changes::move_to_trash(
            self.sources.extensions,
            WorktreeMoveToTrashRequest {
                repository: project.directory.display().to_string(),
                path: item.worktree_path.clone(),
                trash_path: item.path.clone(),
                expected_head: item.head.clone(),
                allowed_ignored_paths: allowed.clone(),
                allow_unpushed: self.allow_unpushed(),
                base_branch: self.worktrees.settings.base.clone(),
            },
        )
        .await?;
        if !response.refusals.is_empty() {
            return Ok(Outcome::Refused(response.refusals, response.message));
        }
        let trashed_at_ms = now().0;
        let grace_ms = u64::try_from(self.worktrees.settings.grace.as_millis()).unwrap_or(u64::MAX);
        let entry = TrashEntry {
            id: last_part(&trash_path),
            project_id: item.project_id.clone(),
            repository: project.directory.clone(),
            path: PathBuf::from(&item.worktree_path),
            trash_path,
            branch: item.branch.clone(),
            head: item.head.clone(),
            trashed_at_ms,
            expires_at_ms: trashed_at_ms.saturating_add(grace_ms),
            deletes_branch: item.deletes_branch,
            allowed_ignored_paths: allowed,
            allow_unpushed: self.allow_unpushed(),
        };
        self.worktrees
            .store
            .add_trash_entry(entry)
            .map_err(|error| format!("the moved worktree is in the trash place, and the worktree file was not saved: {error}"))?;
        self.worktrees.trash_changed.notify_one();
        self.worktrees.sizes.forget(Path::new(&item.worktree_path));
        self.publish(item, WorktreeChange::Trashed, false);
        Ok(Outcome::Done)
    }

    async fn delete(
        &self,
        item: &WorktreePlanItem,
        project: &Described,
    ) -> Result<Outcome, String> {
        let response = git_changes::delete(
            self.sources.extensions,
            WorktreeDeleteRequest {
                repository: project.directory.display().to_string(),
                path: item.worktree_path.clone(),
                expected_head: item.head.clone(),
                allowed_ignored_paths: self.allowed_ignored_paths(item, project),
                allow_unpushed: self.allow_unpushed(),
                delete_branch: item.deletes_branch,
                base_branch: self.worktrees.settings.base.clone(),
            },
        )
        .await?;
        if !response.refusals.is_empty() {
            return Ok(Outcome::Refused(response.refusals, response.message));
        }
        if let Err(error) = self.worktrees.store.forget(Path::new(&item.worktree_path)) {
            warn!(%error, "the worktree file was not saved");
        }
        self.worktrees.sizes.forget(Path::new(&item.worktree_path));
        self.publish(item, WorktreeChange::Removed, response.branch_deleted);
        Ok(Outcome::Done)
    }

    /// The prune of the records of one Project: one call for each item of
    /// the plan that the new plan still has.
    async fn prune(&mut self, items: Vec<WorktreePlanItem>) {
        let project_id = items
            .first()
            .map_or_else(String::new, |item| item.project_id.clone());
        let mut runnable = Vec::new();
        for item in items {
            if item.step() == WorktreePlanStep::Repair {
                let message = item.warnings.join("; ");
                self.result(item, WorktreeCleanOutcome::Refused, message);
            } else if !item.refusals.is_empty() {
                self.result(item, WorktreeCleanOutcome::Refused, String::new());
            } else {
                runnable.push(item);
            }
        }
        if runnable.is_empty() {
            return;
        }
        let Ok(projects) = self
            .worktrees
            .snapshot(&project_id, self.sources, Sizes::Cached)
            .await
        else {
            for item in runnable {
                let message = "the Project has no facts now".to_owned();
                self.result(item, WorktreeCleanOutcome::Changed, message);
            }
            return;
        };
        let fresh: Vec<_> = plan::items(&projects, self.selection)
            .into_iter()
            .filter(|fresh| fresh.step() == WorktreePlanStep::Prune && fresh.refusals.is_empty())
            .map(|fresh| plan::key(&fresh))
            .collect();
        let (current, changed): (Vec<_>, Vec<_>) = runnable
            .into_iter()
            .partition(|item| fresh.contains(&plan::key(item)));
        for item in changed {
            let message = "the record changed after the plan".to_owned();
            self.result(item, WorktreeCleanOutcome::Changed, message);
        }
        let Some(project) = projects.first() else {
            return;
        };
        if current.is_empty() {
            return;
        }
        let pruned = git_changes::prune(
            self.sources.extensions,
            WorktreePruneRequest {
                repository: project.directory.display().to_string(),
                paths: current
                    .iter()
                    .map(|item| item.worktree_path.clone())
                    .collect(),
            },
        )
        .await;
        for item in current {
            match &pruned {
                Ok(response) if response.refusal.is_empty() => {
                    self.publish(&item, WorktreeChange::Pruned, false);
                    self.result(item, WorktreeCleanOutcome::Done, String::new());
                }
                Ok(response) => {
                    let message = response.refusal.clone();
                    self.result(item, WorktreeCleanOutcome::Changed, message);
                }
                Err(message) => {
                    let message = message.clone();
                    self.result(item, WorktreeCleanOutcome::Failed, message);
                }
            }
        }
    }

    /// The ignored paths that may go with the directory: its build output,
    /// and with `include_ignored` the files of the user.
    fn allowed_ignored_paths(&self, item: &WorktreePlanItem, project: &Described) -> Vec<String> {
        let Some(detail) = project
            .wire
            .worktrees
            .iter()
            .zip(&project.details)
            .find(|(worktree, _)| worktree.path == item.worktree_path)
            .map(|(_, detail)| detail)
        else {
            return Vec::new();
        };
        let mut allowed: Vec<String> = detail
            .build_outputs
            .iter()
            .map(|(path, _)| format!("{path}/"))
            .collect();
        if self.selection.include_ignored && !self.selection.automatic {
            allowed.extend(detail.user_paths.iter().cloned());
        }
        allowed
    }

    fn allow_unpushed(&self) -> bool {
        self.selection.allow_unpushed && !self.selection.automatic
    }

    fn publish(&self, item: &WorktreePlanItem, change: WorktreeChange, branch_deleted: bool) {
        self.worktrees.publish(WorktreeChanged {
            project_id: item.project_id.clone(),
            path: item.worktree_path.clone(),
            change: change.into(),
            bytes: item.bytes,
            automatic: self.selection.automatic,
            branch: item.branch.clone(),
            branch_deleted,
        });
    }

    fn result(&mut self, item: WorktreePlanItem, outcome: WorktreeCleanOutcome, message: String) {
        self.results.push(WorktreeCleanResult {
            item: Some(item),
            outcome: outcome.into(),
            message,
        });
    }
}

enum Outcome {
    Done,
    /// The git extension refused the step.
    Refused(Vec<i32>, String),
}

/// A directory on each file system of the worktrees of `items`, by its
/// device, with its free bytes now. The directory is a parent directory of
/// a worktree: the clean moves or removes the worktree itself.
fn free_space(items: &[WorktreePlanItem]) -> HashMap<u64, (PathBuf, u64)> {
    let mut free = HashMap::new();
    for item in items {
        let path = Path::new(&item.worktree_path);
        let Some(directory) = path
            .ancestors()
            .skip(1)
            .find(|directory| directory.exists())
        else {
            continue;
        };
        if let Some(device) = host_facts::device(directory) {
            free.entry(device)
                .or_insert_with(|| (directory.to_path_buf(), host_facts::space(directory).0));
        }
    }
    free
}
