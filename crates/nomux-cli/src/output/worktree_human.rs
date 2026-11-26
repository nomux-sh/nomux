//! The text form of `worktree ls`: one table with a row for each Project and
//! the worktrees below it, a total line, and the reasons why facts are
//! missing.

use std::fmt::Write;
use std::path::Path;

use nomux_client::{
    MergedBy, ProjectWorktrees, Refusal, Worktree, WorktreeFlag, WorktreeSize, WorktreeState,
};

use super::status_human::{duration, now_ms};
use super::table::{Cell, INDENT, Table};
use super::wrapped_line::wrapped;
use crate::workspace_commands::node_path::path_of;
use crate::worktree_commands::WorktreeListing;

const NONE: &str = "-";
/// The mark of a size that the daemon did not measure yet.
const NOT_MEASURED: &str = "?";
/// The mark after a size that is old.
const OLD: &str = "*";
/// The column with the name of a worktree.
const NAME: usize = 0;
const BRANCH: usize = 1;
const BUILD: usize = 4;
const AGE: usize = 5;
const REASON: usize = 6;

pub(super) fn report(listing: &WorktreeListing, width: Option<usize>) -> String {
    if listing.projects.is_empty() {
        return "no projects\n".to_owned();
    }
    let now = now_ms();
    let mut table = Table::new(&[
        "WORKTREE", "BRANCH", "STATE", "SIZE", "BUILD", "AGE", "REASON",
    ])
    .shorten(&[BRANCH, NAME, REASON])
    .omit(&[BUILD, AGE]);
    let mut notes = Vec::new();
    for project in &listing.projects {
        let name = path_of(&listing.workspace.tree, &project.project_id);
        table.push(project_row(&name));
        if let Some(reason) = &project.unavailable_reason {
            notes.push(format!("{name}: {reason}"));
        }
        for worktree in &project.worktrees {
            table.push(worktree_row(worktree, now));
            if let Some(reason) = &worktree.facts_unavailable_reason {
                notes.push(format!("{}: {reason}", worktree.path));
            }
        }
    }
    let mut text = table.render(width);
    text.push_str(&wrapped(&total_line(&listing.projects, now), width));
    for note in notes {
        text.push_str(&wrapped(&note, width));
    }
    text
}

/// Only what a script needs: the path of each worktree.
pub(super) fn quiet_report(listing: &WorktreeListing) -> String {
    listing
        .projects
        .iter()
        .flat_map(|project| &project.worktrees)
        .flat_map(|worktree| [worktree.path.as_str(), "\n"])
        .collect()
}

fn project_row(name: &str) -> Vec<Cell> {
    let mut row = vec![Cell::new(name)];
    row.extend((1..=REASON).map(|_| Cell::new("")));
    row
}

fn worktree_row(worktree: &Worktree, now: u64) -> Vec<Cell> {
    let name = Path::new(&worktree.path).file_name().map_or_else(
        || worktree.path.clone(),
        |name| name.to_string_lossy().into_owned(),
    );
    let size = worktree.size.as_ref();
    vec![
        Cell::new(format!("{INDENT}{name}")),
        Cell::new(worktree.branch.as_deref().unwrap_or(NONE)),
        Cell::new(state_name(worktree.state)),
        Cell::new(size_cell(size, |size| size.total_bytes)),
        Cell::new(size_cell(size, |size| size.build_output_bytes)),
        Cell::new(
            worktree
                .last_activity_at_ms
                .map_or_else(|| NONE.to_owned(), |at| duration(now.saturating_sub(at))),
        ),
        Cell::new(reason(worktree)),
    ]
}

pub(super) fn state_name(state: WorktreeState) -> &'static str {
    match state {
        WorktreeState::Active => "active",
        WorktreeState::Finished => "finished",
        WorktreeState::Stale => "stale",
        WorktreeState::Idle => "idle",
        WorktreeState::Unknown => "unknown",
    }
}

fn size_cell(size: Option<&WorktreeSize>, bytes: impl Fn(&WorktreeSize) -> u64) -> String {
    match size {
        None => NOT_MEASURED.to_owned(),
        Some(size) if size.is_old => format!("{}{OLD}", short_size(bytes(size))),
        Some(size) => short_size(bytes(size)),
    }
}

/// `bytes` in the largest binary unit that gives a value of 1 or more, with
/// one decimal place, and one letter for the unit. The value is cut, not
/// rounded.
fn short_size(bytes: u64) -> String {
    const UNITS: [(u64, char); 4] = [
        (1 << 40, 'T'),
        (1 << 30, 'G'),
        (1 << 20, 'M'),
        (1 << 10, 'K'),
    ];
    let Some((unit, letter)) = UNITS.into_iter().find(|(unit, _)| bytes >= *unit) else {
        return format!("{bytes}B");
    };
    let tenths = u128::from(bytes) * 10 / u128::from(unit);
    format!("{}.{}{letter}", tenths / 10, tenths % 10)
}

/// `yes` and the merge rule, or `no` and the first refusal.
fn reason(worktree: &Worktree) -> String {
    let Some(refusal) = worktree.refusals.first() else {
        return format!("yes: {}", merged_name(worktree.merged_by));
    };
    let text = match refusal {
        Refusal::MainWorktree => "main worktree".to_owned(),
        Refusal::DirectoryMissing => "directory missing".to_owned(),
        Refusal::FactsUnknown => "git facts not known".to_owned(),
        Refusal::ActiveSession => "session running".to_owned(),
        Refusal::ProcessRunning => "process running".to_owned(),
        Refusal::UncommittedChanges => "uncommitted changes".to_owned(),
        Refusal::UnpushedCommits => format!("unpushed {}", worktree.unpushed_commits),
        Refusal::DetachedCommits => "detached commits".to_owned(),
        Refusal::Locked => match &worktree.locked_reason {
            Some(reason) => format!("locked ({reason})"),
            None => "locked".to_owned(),
        },
        Refusal::Submodule => "submodule".to_owned(),
        Refusal::IgnoredUserFiles => {
            format!("ignored files ({})", worktree.ignored_user_paths.join(", "))
        }
        Refusal::OtherFileSystem => "other file system".to_owned(),
        Refusal::NotFinished => "not merged".to_owned(),
        Refusal::Unknown => "unknown reason".to_owned(),
    };
    format!("no: {text}")
}

fn merged_name(merged_by: MergedBy) -> &'static str {
    match merged_by {
        MergedBy::Ancestor => "merged",
        MergedBy::Tree => "merged (same tree)",
        MergedBy::UpstreamGone => "merged (upstream gone)",
        MergedBy::None | MergedBy::Unknown => "finished",
    }
}

/// The sums of the measured sizes, the worktrees that a removal can take,
/// the free space and the age of the last fetch.
fn total_line(projects: &[ProjectWorktrees], now: u64) -> String {
    let worktrees: Vec<&Worktree> = projects
        .iter()
        .flat_map(|project| &project.worktrees)
        .collect();
    let sizes = worktrees
        .iter()
        .filter_map(|worktree| worktree.size.as_ref());
    let total: u64 = sizes.clone().map(|size| size.total_bytes).sum();
    let build: u64 = sizes.map(|size| size.build_output_bytes).sum();
    let removable: Vec<&&Worktree> = worktrees
        .iter()
        .filter(|worktree| worktree.refusals.is_empty())
        .collect();
    let removable_bytes: u64 = removable
        .iter()
        .filter_map(|worktree| worktree.size.as_ref())
        .map(|size| size.total_bytes)
        .sum();
    let mut line = format!(
        "{} worktrees, {} (build output {}), {} can be removed ({})",
        worktrees.len(),
        short_size(total),
        short_size(build),
        removable.len(),
        short_size(removable_bytes),
    );
    if let [project] = projects {
        if let Some(free) = project.free_bytes {
            let _ = write!(line, ", {} free", short_size(free));
        }
        let fetch = project.last_fetch_at_ms.map_or_else(
            || "never".to_owned(),
            |at| format!("{} ago", duration(now.saturating_sub(at))),
        );
        let _ = write!(line, ", last fetch {fetch}");
    }
    let unmeasured = worktrees.iter().any(|worktree| {
        worktree.size.is_none()
            && !worktree.is_bare
            && !worktree.flags.contains(&WorktreeFlag::Missing)
    });
    let old = worktrees
        .iter()
        .any(|worktree| worktree.size.as_ref().is_some_and(|size| size.is_old));
    if unmeasured || old {
        let _ = write!(
            line,
            ". {NOT_MEASURED} not measured yet, {OLD} old: the daemon measures in the background"
        );
    }
    line
}

#[cfg(test)]
mod tests {
    use nomux_client::{NodeKind, Workspace, WorkspaceTree};
    use nomux_core::workspace_tree::Node;

    use super::super::table::assert_fits;
    use super::*;

    const GIBIBYTE: u64 = 1 << 30;

    fn worktree(
        name: &str,
        branch: &str,
        state: WorktreeState,
        refusals: Vec<Refusal>,
    ) -> Worktree {
        Worktree {
            path: format!("/home/user/work/customers/{name}"),
            branch: Some(branch.to_owned()),
            head: Some("8451064c08cc3f4667602ea1139607109cc12da5".to_owned()),
            is_main: false,
            is_bare: false,
            state,
            flags: Vec::new(),
            session_ids: Vec::new(),
            size: Some(WorktreeSize {
                total_bytes: 5 * GIBIBYTE + 700 * (1 << 20),
                build_output_bytes: 5 * GIBIBYTE,
                measured_at_ms: 1,
                is_old: false,
                build_outputs: Vec::new(),
            }),
            last_activity_at_ms: Some(1),
            merged_by: MergedBy::UpstreamGone,
            unpushed_commits: 2,
            refusals,
            ignored_user_paths: vec![
                ".env".to_owned(),
                "local-development-database.db".to_owned(),
            ],
            locked_reason: Some("claude agent agent-a1b2c3d4 (pid 4242)".to_owned()),
            facts_unavailable_reason: None,
        }
    }

    fn listing() -> WorktreeListing {
        let tree = WorkspaceTree::from_nodes([Node {
            id: "p1".to_owned(),
            parent_id: None,
            name: "kubernetes-cluster-configuration".to_owned(),
            kind: NodeKind::Project {
                directory: "/home/user/work/customers/kubernetes-cluster".into(),
            },
        }])
        .expect("a valid tree");
        let mut old = worktree(
            "spike",
            "old-spike",
            WorktreeState::Stale,
            vec![Refusal::IgnoredUserFiles],
        );
        old.size.as_mut().unwrap().is_old = true;
        let worktrees = vec![
            worktree(
                "kubernetes-cluster",
                "main",
                WorktreeState::Active,
                vec![Refusal::MainWorktree],
            ),
            worktree(
                "kubernetes-cluster-authentication-flow",
                "feature/authentication-flow-with-tokens",
                WorktreeState::Finished,
                Vec::new(),
            ),
            worktree(
                "fix",
                "fix-cache",
                WorktreeState::Idle,
                vec![Refusal::UnpushedCommits],
            ),
            old,
            worktree(
                "agent-a1b2c3d4",
                "worktree-agent-a1b2c3d4",
                WorktreeState::Finished,
                vec![Refusal::Locked],
            ),
            Worktree {
                size: None,
                ..worktree(
                    "new",
                    "new",
                    WorktreeState::Idle,
                    vec![Refusal::NotFinished],
                )
            },
        ];
        WorktreeListing {
            workspace: Workspace { revision: 1, tree },
            projects: vec![ProjectWorktrees {
                project_id: "p1".to_owned(),
                unavailable_reason: None,
                base_branch: Some("origin/main".to_owned()),
                last_fetch_at_ms: Some(1),
                free_bytes: Some(14 * GIBIBYTE),
                file_system_bytes: Some(400 * GIBIBYTE),
                worktrees,
            }],
        }
    }

    #[test]
    fn the_table_and_the_total_line_fit_a_terminal_of_60_columns() {
        let text = report(&listing(), Some(60));

        assert_fits(&text, 60);
        assert!(text.contains('\u{2026}'), "{text}");
        assert!(text.contains("STATE"), "{text}");
        assert!(text.contains("6 worktrees"), "{text}");
    }

    #[test]
    fn a_full_line_has_each_column_and_the_marks_of_the_sizes() {
        let text = report(&listing(), None);

        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("WORKTREE"), "{text}");
        assert!(lines[0].ends_with("REASON"), "{text}");
        assert!(text.contains("yes: merged (upstream gone)"), "{text}");
        assert!(text.contains("no: unpushed 2"), "{text}");
        assert!(
            text.contains("no: ignored files (.env, local-development-database.db)"),
            "{text}"
        );
        assert!(
            text.contains("no: locked (claude agent agent-a1b2c3d4 (pid 4242))"),
            "{text}"
        );
        assert!(text.contains("5.6G*"), "{text}");
        assert!(text.contains(" ? "), "{text}");
        assert!(
            text.contains("1 can be removed (5.6G), 14.0G free"),
            "{text}"
        );
    }

    #[test]
    fn a_short_size_has_one_letter_and_is_cut_to_one_decimal_place() {
        assert_eq!(short_size(0), "0B");
        assert_eq!(short_size(1023), "1023B");
        assert_eq!(short_size(1024), "1.0K");
        assert_eq!(short_size((1 << 20) - 1), "1023.9K");
        assert_eq!(short_size(5 * GIBIBYTE + 7 * GIBIBYTE / 10 + 1), "5.7G");
    }
}
