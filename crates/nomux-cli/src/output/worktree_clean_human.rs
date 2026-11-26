//! The text form of `worktree clean`, `worktree restore`, `worktree trash`
//! and `worktree mark`: the plan with each path, its size and its reason,
//! the result of each item, and the trash place.

use std::fmt::Write;
use std::path::Path;

use nomux_client::{
    CleanOutcome, Cleaned, Mark, Plan, PlanItem, PlanStep, TrashEntry, TrashRemoval,
};

use super::status_human::{duration, now_ms};
use super::table::{Cell, Table};
use super::worktree_human::{refusal_name, short_size};
use super::wrapped_line::wrapped;
use crate::worktree_commands::WorktreeReport;

const NONE: &str = "-";
const WORKTREE: usize = 0;
const PATH: usize = 2;
const REASON: usize = 4;

pub(super) fn report(report: &WorktreeReport, width: Option<usize>) -> String {
    match report {
        WorktreeReport::Plan(plan) => {
            let mut text = plan_text(plan, width);
            if plan.items.iter().any(|item| item.refusals.is_empty()) {
                text.push_str(&wrapped(
                    "Nothing changed. Run the command again with --yes to clean these items.",
                    width,
                ));
            }
            text
        }
        WorktreeReport::Cleaned { cleaned, .. } => cleaned_text(cleaned, width),
        WorktreeReport::Trash(entries) => trash_text(entries, width),
        WorktreeReport::Restored(entry) => format!(
            "worktree {} restored from {}\n",
            entry.path, entry.trash_path
        ),
        WorktreeReport::TrashEmptied(removals) => emptied_text(removals),
        WorktreeReport::Marked { path, mark } => match mark {
            Mark::None => format!("worktree {path} has no mark\n"),
            Mark::Finished => format!("worktree {path} marked done\n"),
            Mark::Keep => format!("worktree {path} marked keep\n"),
        },
    }
}

/// Only what a script needs: the paths that a clean deletes or moves, or
/// the identifiers of the trash entries.
pub(super) fn quiet_report(report: &WorktreeReport) -> String {
    match report {
        WorktreeReport::Plan(plan) => lines(
            plan.items
                .iter()
                .filter(|item| item.refusals.is_empty())
                .map(target),
        ),
        WorktreeReport::Cleaned { cleaned, .. } => lines(
            cleaned
                .results
                .iter()
                .filter(|result| result.outcome == CleanOutcome::Done)
                .map(|result| target(&result.item)),
        ),
        WorktreeReport::Trash(entries) => lines(entries.iter().map(|entry| entry.id.clone())),
        WorktreeReport::Restored(entry) => format!("{}\n", entry.path),
        WorktreeReport::TrashEmptied(_) | WorktreeReport::Marked { .. } => String::new(),
    }
}

fn lines(items: impl Iterator<Item = String>) -> String {
    items.fold(String::new(), |mut text, item| {
        text.push_str(&item);
        text.push('\n');
        text
    })
}

/// The path that an item deletes or moves.
fn target(item: &PlanItem) -> String {
    match item.step {
        PlanStep::BuildOutput => item.path.clone().unwrap_or_default(),
        _ => item.worktree_path.clone(),
    }
}

/// The plan: one row for each item, then the sum.
pub(crate) fn plan_text(plan: &Plan, width: Option<usize>) -> String {
    if plan.items.is_empty() {
        return "nothing to clean\n".to_owned();
    }
    let mut table = item_table();
    let mut notes = Vec::new();
    for item in &plan.items {
        table.push(item_row(item, &plan_reason(item)));
        notes.extend(
            item.warnings
                .iter()
                .map(|warning| format!("{}: {warning}", name(&item.worktree_path))),
        );
    }
    notes.sort();
    notes.dedup();
    let runnable = plan
        .items
        .iter()
        .filter(|item| item.refusals.is_empty())
        .count();
    let mut text = table.render(width);
    let mut total = format!(
        "{runnable} of {} items run, {} comes back (upper bound)",
        plan.items.len(),
        short_size(plan.total_bytes)
    );
    if plan.items.iter().any(|item| item.step == PlanStep::Trash) {
        let _ = write!(
            total,
            ", the trash place keeps the worktrees for {}",
            duration(plan.grace_ms)
        );
    }
    total.push('.');
    text.push_str(&wrapped(&total, width));
    for note in notes {
        text.push_str(&wrapped(&note, width));
    }
    text
}

fn cleaned_text(cleaned: &Cleaned, width: Option<usize>) -> String {
    let mut table = item_table();
    for result in &cleaned.results {
        let reason = match result.outcome {
            CleanOutcome::Done => "done".to_owned(),
            CleanOutcome::Changed => format!("skipped: {}", message(result.message.as_deref())),
            CleanOutcome::Refused => refused_reason(&result.item, result.message.as_deref()),
            CleanOutcome::Failed => format!("failed: {}", message(result.message.as_deref())),
            CleanOutcome::Unknown => "unknown result".to_owned(),
        };
        table.push(item_row(&result.item, &reason));
    }
    let done = cleaned
        .results
        .iter()
        .filter(|result| result.outcome == CleanOutcome::Done)
        .count();
    let mut text = table.render(width);
    text.push_str(&wrapped(
        &format!(
            "{done} of {} items done: {} planned, the free space grew by {}.",
            cleaned.results.len(),
            short_size(cleaned.planned_bytes),
            short_size(cleaned.freed_bytes)
        ),
        width,
    ));
    text
}

fn message(message: Option<&str>) -> &str {
    message.unwrap_or("no reason")
}

fn item_table() -> Table {
    Table::new(&["WORKTREE", "STEP", "PATH", "SIZE", "REASON"]).shorten(&[PATH, WORKTREE, REASON])
}

fn item_row(item: &PlanItem, reason: &str) -> Vec<Cell> {
    let worktree = Path::new(&item.worktree_path);
    let path = match (item.step, item.path.as_deref()) {
        (PlanStep::BuildOutput, Some(path)) => Path::new(path).strip_prefix(worktree).map_or_else(
            |_| path.to_owned(),
            |relative| relative.display().to_string(),
        ),
        (PlanStep::Trash, Some(path)) => worktree
            .parent()
            .and_then(|parent| Path::new(path).strip_prefix(parent).ok())
            .map_or_else(
                || path.to_owned(),
                |relative| relative.display().to_string(),
            ),
        (_, Some(path)) => path.to_owned(),
        (_, None) => NONE.to_owned(),
    };
    let size = if item.bytes == 0 && item.step != PlanStep::BuildOutput {
        NONE.to_owned()
    } else {
        short_size(item.bytes)
    };
    vec![
        Cell::new(name(&item.worktree_path)),
        Cell::new(step_name(item.step)),
        Cell::new(path),
        Cell::new(size),
        Cell::new(reason),
    ]
}

fn name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn step_name(step: PlanStep) -> &'static str {
    match step {
        PlanStep::BuildOutput => "build output",
        PlanStep::Trash => "to trash",
        PlanStep::Delete => "delete now",
        PlanStep::Prune => "prune record",
        PlanStep::Repair => "repair",
        PlanStep::Unknown => "unknown",
    }
}

/// `yes` and what the item keeps, or `no` and the first refusal.
fn plan_reason(item: &PlanItem) -> String {
    if !item.refusals.is_empty() {
        return refused_reason(item, None);
    }
    match item.step {
        PlanStep::BuildOutput => format!("yes: {}", item.rule.as_deref().unwrap_or("build output")),
        PlanStep::Trash | PlanStep::Delete => match (&item.branch, item.deletes_branch) {
            (Some(branch), true) => format!("yes: branch {branch} deleted after"),
            (Some(branch), false) => format!("yes: branch {branch} stays"),
            (None, _) => "yes".to_owned(),
        },
        PlanStep::Prune => "yes: the directory is missing".to_owned(),
        PlanStep::Repair => "no: a proposal, see below".to_owned(),
        PlanStep::Unknown => "yes".to_owned(),
    }
}

fn refused_reason(item: &PlanItem, message: Option<&str>) -> String {
    let Some(refusal) = item.refusals.first() else {
        return format!("refused: {}", message.unwrap_or("no reason"));
    };
    let mut text = format!("no: {}", refusal_name(*refusal));
    if *refusal == nomux_client::Refusal::IgnoredUserFiles && !item.ignored_user_paths.is_empty() {
        let _ = write!(text, " ({})", item.ignored_user_paths.join(", "));
    }
    text
}

fn trash_text(entries: &[TrashEntry], width: Option<usize>) -> String {
    if entries.is_empty() {
        return "the trash place is empty\n".to_owned();
    }
    let now = now_ms();
    let mut table =
        Table::new(&["ID", "WORKTREE", "BRANCH", "MOVED", "REMOVED IN"]).shorten(&[1, 0, 2]);
    for entry in entries {
        table.push(vec![
            Cell::new(entry.id.as_str()),
            Cell::new(entry.path.as_str()),
            Cell::new(entry.branch.as_deref().unwrap_or(NONE)),
            Cell::new(format!(
                "{} ago",
                duration(now.saturating_sub(entry.trashed_at_ms))
            )),
            Cell::new(duration(entry.expires_at_ms.saturating_sub(now))),
        ]);
    }
    table.render(width)
}

fn emptied_text(removals: &[TrashRemoval]) -> String {
    if removals.is_empty() {
        return "the trash place is empty\n".to_owned();
    }
    removals.iter().fold(String::new(), |mut text, removal| {
        let entry = &removal.entry;
        let _ = if removal.removed {
            match (
                &entry.branch,
                removal.branch_deleted,
                &removal.branch_kept_reason,
            ) {
                (Some(branch), true, _) => {
                    writeln!(text, "{}: removed, branch {branch} deleted", entry.path)
                }
                (_, _, Some(reason)) => writeln!(text, "{}: removed, {reason}", entry.path),
                _ => writeln!(text, "{}: removed", entry.path),
            }
        } else {
            writeln!(
                text,
                "{}: stays: {}",
                entry.path,
                message(removal.message.as_deref())
            )
        };
        text
    })
}
