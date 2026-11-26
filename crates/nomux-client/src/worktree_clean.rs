//! The clean of the worktrees: the plan that a dry run shows, the clean
//! that runs one plan, the marks, and the trash place.

use nomux_proto::wire::v1::{
    self as wire, WorktreeCleanRequest, WorktreeEmptyTrashRequest, WorktreeMarkRequest,
    WorktreePlanRequest, WorktreeRestoreRequest, WorktreeTrashRequest, request, response,
};

use crate::client::unexpected_result;
use crate::worktree::{Mark, Refusal, refusal, text};
use crate::{Client, ClientError};

/// What a clean removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanAction {
    /// The build output of each linked worktree.
    BuildOutput,
    /// Each finished worktree: its build output, then its directory to the
    /// trash place; and the records of missing directories.
    Finished,
    /// The build output of each stale worktree.
    Stale,
    /// Only the records of git of missing directories.
    Prune,
}

/// The request of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the options of the plan, each one independent"
)]
pub struct CleanRequest {
    /// One Project, or each Project with `None`.
    pub project_id: Option<String>,
    /// What the clean removes.
    pub action: CleanAction,
    /// Only these worktrees, by their absolute path. Empty: each one.
    pub worktree_paths: Vec<String>,
    /// Also delete each branch that is merged by the strict rule.
    pub delete_branch: bool,
    /// Move a worktree with commits that are not pushed.
    pub allow_unpushed: bool,
    /// Move the ignored files of the user with the directory.
    pub include_ignored: bool,
    /// Remove now, without the grace time.
    pub now: bool,
}

/// One step of a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStep {
    /// Deletes one build output directory.
    BuildOutput,
    /// Moves the directory to the trash place.
    Trash,
    /// Removes the worktree now.
    Delete,
    /// Removes the records of missing directories.
    Prune,
    /// A proposal: `git worktree repair <path>`.
    Repair,
    /// A step from a later protocol version.
    Unknown,
}

/// One item of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    /// The identifier of the Project.
    pub project_id: String,
    /// The absolute path of the worktree.
    pub worktree_path: String,
    /// What the item does.
    pub step: PlanStep,
    /// The directory of a build output step, the path in the trash place,
    /// or the directory of a repair proposal.
    pub path: Option<String>,
    /// The build-output rule.
    pub rule: Option<String>,
    /// The allocated bytes that the step frees: an upper bound.
    pub bytes: u64,
    /// The branch of the worktree.
    pub branch: Option<String>,
    /// The commit of `HEAD` that the plan saw.
    pub head: Option<String>,
    /// Why the item does not run. Empty: it runs.
    pub refusals: Vec<Refusal>,
    /// Facts that the user reads before a clean.
    pub warnings: Vec<String>,
    /// The removal also deletes the branch.
    pub deletes_branch: bool,
    /// Why the branch stays.
    pub branch_kept_reason: Option<String>,
    /// The ignored files of the user.
    pub ignored_user_paths: Vec<String>,
}

/// A plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The identifier for [`Client::clean_worktrees`].
    pub plan_id: String,
    /// The items, for each worktree its build output first.
    pub items: Vec<PlanItem>,
    /// The bytes of the items that run.
    pub total_bytes: u64,
    /// The grace time of the trash place in milliseconds.
    pub grace_ms: u64,
}

/// What a clean did with one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanOutcome {
    /// The step ran.
    Done,
    /// The facts changed after the plan.
    Changed,
    /// The item has a refusal, or the git extension refused it.
    Refused,
    /// The step failed.
    Failed,
    /// An outcome from a later protocol version.
    Unknown,
}

/// The result of one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanResult {
    /// The item of the plan.
    pub item: PlanItem,
    /// What the clean did.
    pub outcome: CleanOutcome,
    /// What happened. `None` for [`CleanOutcome::Done`].
    pub message: Option<String>,
}

/// The result of a clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cleaned {
    /// One result for each item, in the sequence of the plan.
    pub results: Vec<CleanResult>,
    /// The bytes of the items that ran, as the plan counted them.
    pub planned_bytes: u64,
    /// The increase of the free space during the clean.
    pub freed_bytes: u64,
}

/// One worktree in the trash place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashEntry {
    /// The identifier of the entry.
    pub id: String,
    /// The identifier of the Project.
    pub project_id: String,
    /// The directory before the move.
    pub path: String,
    /// The path in the trash place.
    pub trash_path: String,
    /// The branch of the worktree.
    pub branch: Option<String>,
    /// The commit of `HEAD` at the move.
    pub head: Option<String>,
    /// When the directory moved: milliseconds after the Unix epoch.
    pub trashed_at_ms: u64,
    /// The end of the grace time.
    pub expires_at_ms: u64,
    /// The removal also deletes the branch.
    pub deletes_branch: bool,
}

/// What the removal of one trash entry did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRemoval {
    /// The entry.
    pub entry: TrashEntry,
    /// `true` when the worktree was removed.
    pub removed: bool,
    /// Why the worktree stays.
    pub message: Option<String>,
    /// `true` when the branch was deleted.
    pub branch_deleted: bool,
    /// Why the branch stays.
    pub branch_kept_reason: Option<String>,
}

impl Client {
    /// The plan of a clean. It changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier or a
    /// path is no worktree, and [`ClientError::Failed`] for a Group or a
    /// daemon that is older than this request.
    pub async fn plan_clean(&mut self, clean: &CleanRequest) -> Result<Plan, ClientError> {
        let request = WorktreePlanRequest {
            project_id: clean.project_id.clone().unwrap_or_default(),
            action: match clean.action {
                CleanAction::BuildOutput => wire::WorktreeCleanAction::BuildOutput,
                CleanAction::Finished => wire::WorktreeCleanAction::Finished,
                CleanAction::Stale => wire::WorktreeCleanAction::Stale,
                CleanAction::Prune => wire::WorktreeCleanAction::Prune,
            }
            .into(),
            worktree_paths: clean.worktree_paths.clone(),
            delete_branch: clean.delete_branch,
            allow_unpushed: clean.allow_unpushed,
            include_ignored: clean.include_ignored,
            now: clean.now,
        };
        match self.call(request::Body::WorktreePlan(request)).await? {
            response::Result::WorktreePlan(plan) => Ok(Plan {
                plan_id: plan.plan_id,
                items: plan.items.into_iter().map(PlanItem::from).collect(),
                total_bytes: plan.total_bytes,
                grace_ms: plan.grace_ms,
            }),
            _ => Err(unexpected_result()),
        }
    }

    /// Runs the items of the plan `plan_id`.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] for a plan that the daemon does not
    /// have, and [`ClientError::Failed`] while a different clean runs.
    pub async fn clean_worktrees(&mut self, plan_id: &str) -> Result<Cleaned, ClientError> {
        let request = WorktreeCleanRequest {
            plan_id: plan_id.to_owned(),
        };
        match self.call(request::Body::WorktreeClean(request)).await? {
            response::Result::WorktreeClean(cleaned) => Ok(Cleaned {
                results: cleaned
                    .results
                    .into_iter()
                    .map(|result| CleanResult {
                        outcome: outcome(result.outcome),
                        item: PlanItem::from(result.item.unwrap_or_default()),
                        message: text(result.message),
                    })
                    .collect(),
                planned_bytes: cleaned.planned_bytes,
                freed_bytes: cleaned.freed_bytes,
            }),
            _ => Err(unexpected_result()),
        }
    }

    /// Sets the mark of the worktree `path`, or removes it with
    /// [`Mark::None`].
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if `path` is no worktree of a
    /// Project.
    pub async fn mark_worktree(&mut self, path: &str, mark: Mark) -> Result<(), ClientError> {
        let request = WorktreeMarkRequest {
            path: path.to_owned(),
            mark: match mark {
                Mark::None => wire::WorktreeMark::Unspecified,
                Mark::Finished => wire::WorktreeMark::Finished,
                Mark::Keep => wire::WorktreeMark::Keep,
            }
            .into(),
        };
        match self.call(request::Body::WorktreeMark(request)).await? {
            response::Result::WorktreeMark(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    /// The entries of the trash place of one Project, or of each Project.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier.
    pub async fn trash_entries(
        &mut self,
        project_id: Option<&str>,
    ) -> Result<Vec<TrashEntry>, ClientError> {
        let request = WorktreeTrashRequest {
            project_id: project_id.unwrap_or_default().to_owned(),
        };
        match self.call(request::Body::WorktreeTrash(request)).await? {
            response::Result::WorktreeTrash(trash) => {
                Ok(trash.entries.into_iter().map(TrashEntry::from).collect())
            }
            _ => Err(unexpected_result()),
        }
    }

    /// Moves the worktree `name` back from the trash place: its entry
    /// identifier, its path, or the last part of its path.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no entry has the name, and
    /// [`ClientError::Failed`] if the directory exists.
    pub async fn restore_worktree(&mut self, name: &str) -> Result<TrashEntry, ClientError> {
        let request = WorktreeRestoreRequest {
            name: name.to_owned(),
        };
        match self.call(request::Body::WorktreeRestore(request)).await? {
            response::Result::WorktreeRestore(restored) => {
                Ok(TrashEntry::from(restored.entry.unwrap_or_default()))
            }
            _ => Err(unexpected_result()),
        }
    }

    /// Removes the trash entries `ids` now, or each entry.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if an identifier is no entry.
    pub async fn empty_trash(&mut self, ids: &[String]) -> Result<Vec<TrashRemoval>, ClientError> {
        let request = WorktreeEmptyTrashRequest { ids: ids.to_vec() };
        match self
            .call(request::Body::WorktreeEmptyTrash(request))
            .await?
        {
            response::Result::WorktreeEmptyTrash(emptied) => Ok(emptied
                .removals
                .into_iter()
                .map(|removal| TrashRemoval {
                    entry: TrashEntry::from(removal.entry.unwrap_or_default()),
                    removed: removal.removed,
                    message: text(removal.message),
                    branch_deleted: removal.branch_deleted,
                    branch_kept_reason: text(removal.branch_kept_reason),
                })
                .collect()),
            _ => Err(unexpected_result()),
        }
    }
}

impl From<wire::WorktreePlanItem> for PlanItem {
    fn from(item: wire::WorktreePlanItem) -> Self {
        Self {
            step: match wire::WorktreePlanStep::try_from(item.step) {
                Ok(wire::WorktreePlanStep::BuildOutput) => PlanStep::BuildOutput,
                Ok(wire::WorktreePlanStep::Trash) => PlanStep::Trash,
                Ok(wire::WorktreePlanStep::Delete) => PlanStep::Delete,
                Ok(wire::WorktreePlanStep::Prune) => PlanStep::Prune,
                Ok(wire::WorktreePlanStep::Repair) => PlanStep::Repair,
                Ok(wire::WorktreePlanStep::Unspecified) | Err(_) => PlanStep::Unknown,
            },
            refusals: item.refusals.iter().map(|value| refusal(*value)).collect(),
            project_id: item.project_id,
            worktree_path: item.worktree_path,
            path: text(item.path),
            rule: text(item.rule),
            bytes: item.bytes,
            branch: text(item.branch),
            head: text(item.head),
            warnings: item.warnings,
            deletes_branch: item.deletes_branch,
            branch_kept_reason: text(item.branch_kept_reason),
            ignored_user_paths: item.ignored_user_paths,
        }
    }
}

impl From<wire::WorktreeTrashEntry> for TrashEntry {
    fn from(entry: wire::WorktreeTrashEntry) -> Self {
        Self {
            id: entry.id,
            project_id: entry.project_id,
            path: entry.path,
            trash_path: entry.trash_path,
            branch: text(entry.branch),
            head: text(entry.head),
            trashed_at_ms: entry.trashed_at_ms,
            expires_at_ms: entry.expires_at_ms,
            deletes_branch: entry.deletes_branch,
        }
    }
}

fn outcome(value: i32) -> CleanOutcome {
    match wire::WorktreeCleanOutcome::try_from(value) {
        Ok(wire::WorktreeCleanOutcome::Done) => CleanOutcome::Done,
        Ok(wire::WorktreeCleanOutcome::Changed) => CleanOutcome::Changed,
        Ok(wire::WorktreeCleanOutcome::Refused) => CleanOutcome::Refused,
        Ok(wire::WorktreeCleanOutcome::Failed) => CleanOutcome::Failed,
        Ok(wire::WorktreeCleanOutcome::Unspecified) | Err(_) => CleanOutcome::Unknown,
    }
}
