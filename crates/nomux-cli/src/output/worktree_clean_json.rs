//! The JSON form of `worktree clean`, `worktree restore`, `worktree trash`
//! and `worktree mark`. `docs/cli.md` has the schema.

use nomux_client::{
    CleanOutcome, CleanResult, Mark, Plan, PlanItem, PlanStep, TrashEntry, TrashRemoval,
};
use serde::Serialize;

use super::worktree_json::refusal_name;
use crate::worktree_commands::WorktreeReport;

/// Each variant is one top-level field of the document.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WorktreeBody<'a> {
    Plan(PlanDocument<'a>),
    Clean(CleanDocument<'a>),
    Trash(Vec<TrashDocument<'a>>),
    Restored(TrashDocument<'a>),
    TrashEmptied(Vec<RemovalDocument<'a>>),
    Marked(MarkedDocument<'a>),
}

impl<'a> WorktreeBody<'a> {
    pub(super) fn of(report: &'a WorktreeReport) -> Self {
        match report {
            WorktreeReport::Plan(plan) => Self::Plan(PlanDocument::from(plan)),
            WorktreeReport::Cleaned { plan, cleaned } => Self::Clean(CleanDocument {
                plan_id: &plan.plan_id,
                planned_bytes: cleaned.planned_bytes,
                freed_bytes: cleaned.freed_bytes,
                results: cleaned.results.iter().map(ResultDocument::from).collect(),
            }),
            WorktreeReport::Trash(entries) => {
                Self::Trash(entries.iter().map(TrashDocument::from).collect())
            }
            WorktreeReport::Restored(entry) => Self::Restored(TrashDocument::from(entry)),
            WorktreeReport::TrashEmptied(removals) => {
                Self::TrashEmptied(removals.iter().map(RemovalDocument::from).collect())
            }
            WorktreeReport::Marked { path, mark } => Self::Marked(MarkedDocument {
                path,
                mark: match mark {
                    Mark::None => None,
                    Mark::Finished => Some("done"),
                    Mark::Keep => Some("keep"),
                },
            }),
        }
    }
}

#[derive(Serialize)]
pub(super) struct PlanDocument<'a> {
    id: &'a str,
    total_bytes: u64,
    grace_ms: u64,
    items: Vec<ItemDocument<'a>>,
}

impl<'a> From<&'a Plan> for PlanDocument<'a> {
    fn from(plan: &'a Plan) -> Self {
        Self {
            id: &plan.plan_id,
            total_bytes: plan.total_bytes,
            grace_ms: plan.grace_ms,
            items: plan.items.iter().map(ItemDocument::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct ItemDocument<'a> {
    project_id: &'a str,
    worktree: &'a str,
    step: &'static str,
    path: Option<&'a str>,
    rule: Option<&'a str>,
    bytes: u64,
    branch: Option<&'a str>,
    head: Option<&'a str>,
    runs: bool,
    refusals: Vec<&'static str>,
    warnings: &'a [String],
    deletes_branch: bool,
    branch_kept_reason: Option<&'a str>,
    ignored_user_paths: &'a [String],
}

impl<'a> From<&'a PlanItem> for ItemDocument<'a> {
    fn from(item: &'a PlanItem) -> Self {
        Self {
            project_id: &item.project_id,
            worktree: &item.worktree_path,
            step: match item.step {
                PlanStep::BuildOutput => "build-output",
                PlanStep::Trash => "trash",
                PlanStep::Delete => "delete",
                PlanStep::Prune => "prune",
                PlanStep::Repair => "repair",
                PlanStep::Unknown => "unknown",
            },
            path: item.path.as_deref(),
            rule: item.rule.as_deref(),
            bytes: item.bytes,
            branch: item.branch.as_deref(),
            head: item.head.as_deref(),
            runs: item.refusals.is_empty(),
            refusals: item
                .refusals
                .iter()
                .map(|refusal| refusal_name(*refusal))
                .collect(),
            warnings: &item.warnings,
            deletes_branch: item.deletes_branch,
            branch_kept_reason: item.branch_kept_reason.as_deref(),
            ignored_user_paths: &item.ignored_user_paths,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CleanDocument<'a> {
    plan_id: &'a str,
    planned_bytes: u64,
    freed_bytes: u64,
    results: Vec<ResultDocument<'a>>,
}

#[derive(Serialize)]
struct ResultDocument<'a> {
    #[serde(flatten)]
    item: ItemDocument<'a>,
    outcome: &'static str,
    message: Option<&'a str>,
}

impl<'a> From<&'a CleanResult> for ResultDocument<'a> {
    fn from(result: &'a CleanResult) -> Self {
        Self {
            item: ItemDocument::from(&result.item),
            outcome: match result.outcome {
                CleanOutcome::Done => "done",
                CleanOutcome::Changed => "changed",
                CleanOutcome::Refused => "refused",
                CleanOutcome::Failed => "failed",
                CleanOutcome::Unknown => "unknown",
            },
            message: result.message.as_deref(),
        }
    }
}

#[derive(Serialize)]
pub(super) struct TrashDocument<'a> {
    id: &'a str,
    project_id: &'a str,
    path: &'a str,
    trash_path: &'a str,
    branch: Option<&'a str>,
    head: Option<&'a str>,
    trashed_at: u64,
    expires_at: u64,
    deletes_branch: bool,
}

impl<'a> From<&'a TrashEntry> for TrashDocument<'a> {
    fn from(entry: &'a TrashEntry) -> Self {
        Self {
            id: &entry.id,
            project_id: &entry.project_id,
            path: &entry.path,
            trash_path: &entry.trash_path,
            branch: entry.branch.as_deref(),
            head: entry.head.as_deref(),
            trashed_at: entry.trashed_at_ms,
            expires_at: entry.expires_at_ms,
            deletes_branch: entry.deletes_branch,
        }
    }
}

#[derive(Serialize)]
pub(super) struct RemovalDocument<'a> {
    #[serde(flatten)]
    entry: TrashDocument<'a>,
    removed: bool,
    message: Option<&'a str>,
    branch_deleted: bool,
    branch_kept_reason: Option<&'a str>,
}

impl<'a> From<&'a TrashRemoval> for RemovalDocument<'a> {
    fn from(removal: &'a TrashRemoval) -> Self {
        Self {
            entry: TrashDocument::from(&removal.entry),
            removed: removal.removed,
            message: removal.message.as_deref(),
            branch_deleted: removal.branch_deleted,
            branch_kept_reason: removal.branch_kept_reason.as_deref(),
        }
    }
}

#[derive(Serialize)]
pub(super) struct MarkedDocument<'a> {
    path: &'a str,
    mark: Option<&'static str>,
}
