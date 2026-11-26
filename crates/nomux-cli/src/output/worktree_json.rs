//! The JSON form of `worktree ls`. `docs/cli.md` has the schema.

use nomux_client::{
    BuildOutput, Mark, MergedBy, ProjectWorktrees, Refusal, Worktree, WorktreeFlag, WorktreeSize,
};
use serde::Serialize;

use super::worktree_human::state_name;
use crate::workspace_commands::node_path::path_of;
use crate::worktree_commands::WorktreeListing;

#[derive(Serialize)]
pub(super) struct WorktreesDocument<'a> {
    projects: Vec<ProjectDocument<'a>>,
}

impl<'a> WorktreesDocument<'a> {
    pub(super) fn of(listing: &'a WorktreeListing) -> Self {
        Self {
            projects: listing
                .projects
                .iter()
                .map(|project| {
                    ProjectDocument::new(
                        project,
                        path_of(&listing.workspace.tree, &project.project_id),
                    )
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct ProjectDocument<'a> {
    id: &'a str,
    path: String,
    unavailable_reason: Option<&'a str>,
    base_branch: Option<&'a str>,
    last_fetch_at: Option<u64>,
    free_bytes: Option<u64>,
    file_system_bytes: Option<u64>,
    worktrees: Vec<WorktreeDocument<'a>>,
}

impl<'a> ProjectDocument<'a> {
    fn new(project: &'a ProjectWorktrees, path: String) -> Self {
        Self {
            id: &project.project_id,
            path,
            unavailable_reason: project.unavailable_reason.as_deref(),
            base_branch: project.base_branch.as_deref(),
            last_fetch_at: project.last_fetch_at_ms,
            free_bytes: project.free_bytes,
            file_system_bytes: project.file_system_bytes,
            worktrees: project
                .worktrees
                .iter()
                .map(WorktreeDocument::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct WorktreeDocument<'a> {
    path: &'a str,
    branch: Option<&'a str>,
    head: Option<&'a str>,
    is_main: bool,
    is_bare: bool,
    state: &'static str,
    flags: Vec<&'static str>,
    sessions: &'a [String],
    size: Option<SizeDocument<'a>>,
    last_activity_at: Option<u64>,
    merged_by: Option<&'static str>,
    unpushed_commits: u32,
    removable: bool,
    refusals: Vec<&'static str>,
    ignored_user_paths: &'a [String],
    locked_reason: Option<&'a str>,
    facts_unavailable_reason: Option<&'a str>,
    tool: Option<&'a str>,
    mark: Option<&'static str>,
    trash_id: Option<&'a str>,
}

impl<'a> From<&'a Worktree> for WorktreeDocument<'a> {
    fn from(worktree: &'a Worktree) -> Self {
        Self {
            path: &worktree.path,
            branch: worktree.branch.as_deref(),
            head: worktree.head.as_deref(),
            is_main: worktree.is_main,
            is_bare: worktree.is_bare,
            state: state_name(worktree.state),
            flags: worktree.flags.iter().map(|flag| flag_name(*flag)).collect(),
            sessions: &worktree.session_ids,
            size: worktree.size.as_ref().map(SizeDocument::from),
            last_activity_at: worktree.last_activity_at_ms,
            merged_by: merged_by_name(worktree.merged_by),
            unpushed_commits: worktree.unpushed_commits,
            removable: worktree.refusals.is_empty(),
            refusals: worktree
                .refusals
                .iter()
                .map(|refusal| refusal_name(*refusal))
                .collect(),
            ignored_user_paths: &worktree.ignored_user_paths,
            locked_reason: worktree.locked_reason.as_deref(),
            facts_unavailable_reason: worktree.facts_unavailable_reason.as_deref(),
            tool: worktree.tool.as_deref(),
            mark: match worktree.mark {
                Mark::None => None,
                Mark::Finished => Some("done"),
                Mark::Keep => Some("keep"),
            },
            trash_id: worktree.trash_id.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct SizeDocument<'a> {
    total_bytes: u64,
    build_output_bytes: u64,
    measured_at: u64,
    is_old: bool,
    /// The walk counts a hard link one time, but it does not see blocks
    /// that two files share (a reflink): the sizes are an upper bound.
    shared_extents: &'static str,
    build_outputs: Vec<BuildOutputDocument<'a>>,
}

impl<'a> From<&'a WorktreeSize> for SizeDocument<'a> {
    fn from(size: &'a WorktreeSize) -> Self {
        Self {
            total_bytes: size.total_bytes,
            build_output_bytes: size.build_output_bytes,
            measured_at: size.measured_at_ms,
            is_old: size.is_old,
            shared_extents: "unknown",
            build_outputs: size
                .build_outputs
                .iter()
                .map(BuildOutputDocument::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct BuildOutputDocument<'a> {
    path: &'a str,
    rule: &'a str,
    bytes: u64,
}

impl<'a> From<&'a BuildOutput> for BuildOutputDocument<'a> {
    fn from(output: &'a BuildOutput) -> Self {
        Self {
            path: &output.path,
            rule: &output.rule,
            bytes: output.bytes,
        }
    }
}

fn flag_name(flag: WorktreeFlag) -> &'static str {
    match flag {
        WorktreeFlag::Dirty => "dirty",
        WorktreeFlag::Unpushed => "unpushed",
        WorktreeFlag::DetachedCommits => "detached-commits",
        WorktreeFlag::Locked => "locked",
        WorktreeFlag::Submodule => "submodule",
        WorktreeFlag::IgnoredUserFiles => "ignored-user-files",
        WorktreeFlag::Busy => "busy",
        WorktreeFlag::Missing => "missing",
        WorktreeFlag::OtherFileSystem => "other-file-system",
        WorktreeFlag::UpstreamGone => "upstream-gone",
        WorktreeFlag::Unknown => "unknown",
    }
}

fn merged_by_name(merged_by: MergedBy) -> Option<&'static str> {
    match merged_by {
        MergedBy::None => None,
        MergedBy::Ancestor => Some("ancestor"),
        MergedBy::Tree => Some("tree"),
        MergedBy::UpstreamGone => Some("upstream-gone"),
        MergedBy::Unknown => Some("unknown"),
    }
}

pub(super) fn refusal_name(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::MainWorktree => "main-worktree",
        Refusal::DirectoryMissing => "directory-missing",
        Refusal::FactsUnknown => "facts-unknown",
        Refusal::ActiveSession => "active-session",
        Refusal::ProcessRunning => "process-running",
        Refusal::UncommittedChanges => "uncommitted-changes",
        Refusal::UnpushedCommits => "unpushed-commits",
        Refusal::DetachedCommits => "detached-commits",
        Refusal::Locked => "locked",
        Refusal::Submodule => "submodule",
        Refusal::IgnoredUserFiles => "ignored-user-files",
        Refusal::OtherFileSystem => "other-file-system",
        Refusal::NotFinished => "not-finished",
        Refusal::Kept => "kept",
        Refusal::NoTrashPlace => "no-trash-place",
        Refusal::InTrash => "in-trash",
        Refusal::Unknown => "unknown",
    }
}
