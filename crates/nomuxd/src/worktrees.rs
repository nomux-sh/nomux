//! The worktrees of the Projects of the host: their state, their flags,
//! their size and the reasons why a removal of each one is refused.
//! `spec/worktree.md` of the `proto` repository has the rules, and
//! `docs/worktrees.md` describes this part. This version only reads.
//!
//! - `facts`: the git facts, from the git extension.
//! - `build_output`: the rules that decide which ignored directory is build
//!   output.
//! - `state`: the state, the flags and the refusals.
//! - `host_facts`: the processes, the devices and the free space.
//! - `sizes`, `size_walk`: the sizes in memory and the walk that measures
//!   them.

mod build_output;
mod facts;
mod host_facts;
mod size_walk;
mod sizes;
mod state;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nomux_ext_host::ExtensionHost;
use nomux_proto::wire::v1::{
    Error, ErrorCode, ProjectWorktrees, WorktreeFacts, WorktreeFactsResponse, WorktreeFlag,
    WorktreeInfo, WorktreeListRequest, WorktreeListResponse,
};
use tokio::task::spawn_blocking;

use self::size_walk::WalkTarget;
use self::sizes::{ProjectWalk, SizeCache};
use self::state::{HostView, judge};
use crate::clock::now;
use crate::sessions::Sessions;
use crate::workspace::Workspace;

/// The largest number of ignored paths of the user in one worktree of a
/// response.
const MAX_USER_PATHS: usize = 32;

#[derive(Default)]
pub(crate) struct Worktrees {
    sizes: Arc<SizeCache>,
}

/// What the list needs of the daemon.
pub(crate) struct Sources<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) sessions: &'a Sessions,
    pub(crate) extensions: &'a ExtensionHost,
}

/// One session that is not ended, with its working directory.
struct LiveSession {
    id: String,
    directory: PathBuf,
}

impl Worktrees {
    /// `Worktree.List`.
    pub(crate) async fn list(
        &self,
        request: &WorktreeListRequest,
        sources: Sources<'_>,
    ) -> Result<WorktreeListResponse, Error> {
        let projects = sources.workspace.projects(&request.project_id)?;
        let sessions = live_sessions(sources.sessions);
        let mut listed = Vec::new();
        for (project_id, directory) in projects {
            let facts = facts::of_project(sources.extensions, &directory).await;
            listed.push((project_id, directory, facts));
        }
        let mut described = spawn_blocking(move || describe_all(listed, &sessions))
            .await
            .map_err(|error| Error {
                code: ErrorCode::Failed.into(),
                message: format!("the worktrees were not read: {error}"),
            })?;
        let live: HashSet<String> = std::mem::take(&mut described.live_sessions);
        let mut walks = Vec::new();
        for project in &mut described.projects {
            walks.extend(self.add_sizes(project, &live));
        }
        if request.wait_for_sizes && !walks.is_empty() {
            for walk in walks {
                self.sizes.measure(walk).await;
            }
            for project in &mut described.projects {
                self.add_sizes(project, &live);
            }
        } else {
            for walk in walks {
                self.sizes.measure_later(walk);
            }
        }
        Ok(WorktreeListResponse {
            projects: described
                .projects
                .into_iter()
                .map(|project| project.wire)
                .collect(),
        })
    }

    /// Puts the size that the cache has into each worktree of `project`.
    /// Returns the walk for the worktrees whose size is absent or old.
    fn add_sizes(&self, project: &mut Described, live: &HashSet<String>) -> Option<ProjectWalk> {
        let roots: HashSet<&Path> = project
            .targets
            .iter()
            .map(|target| target.root.as_path())
            .collect();
        self.sizes.keep_only(&project.directory, &roots);
        let mut needs_walk = false;
        for (worktree, target) in project.wire.worktrees.iter_mut().zip(&project.target_of) {
            let Some(index) = target else {
                continue;
            };
            let (size, is_old) = self.sizes.size_of(&project.targets[*index].root, live);
            worktree.size = size;
            needs_walk |= is_old;
        }
        needs_walk.then(|| ProjectWalk {
            project: project.directory.clone(),
            targets: project.targets.clone(),
            sessions: project
                .target_of
                .iter()
                .zip(&project.wire.worktrees)
                .filter(|(target, _)| target.is_some())
                .map(|(_, worktree)| worktree.session_ids.clone())
                .collect(),
        })
    }
}

fn live_sessions(sessions: &Sessions) -> Vec<LiveSession> {
    sessions
        .list(false)
        .into_iter()
        .filter_map(|session| {
            let directory = session
                .resolved_working_directory
                .or_else(|| session.metadata.map(|metadata| metadata.working_directory))?;
            Some(LiveSession {
                id: session.record.id.to_string(),
                directory,
            })
        })
        .collect()
}

/// The Projects of a response before the sizes.
struct DescribedProjects {
    projects: Vec<Described>,
    live_sessions: HashSet<String>,
}

struct Described {
    directory: PathBuf,
    wire: ProjectWorktrees,
    /// The worktrees to measure: each one whose directory exists.
    targets: Vec<WalkTarget>,
    /// For each worktree of `wire`, its index in `targets`.
    target_of: Vec<Option<usize>>,
}

/// Reads the host for each Project and applies the rules. Runs on a thread
/// that can wait: it reads the process table and the worktrees.
fn describe_all(
    listed: Vec<(String, PathBuf, Result<WorktreeFactsResponse, String>)>,
    sessions: &[LiveSession],
) -> DescribedProjects {
    let host = HostNow {
        sessions,
        processes: host_facts::process_directories(),
        now_ms: now().0,
    };
    DescribedProjects {
        projects: listed
            .into_iter()
            .map(|(id, directory, facts)| describe_project(id, directory, facts, &host))
            .collect(),
        live_sessions: sessions.iter().map(|session| session.id.clone()).collect(),
    }
}

/// What a list reads of the host one time for all Projects.
struct HostNow<'a> {
    sessions: &'a [LiveSession],
    /// The current directory of each process.
    processes: Vec<PathBuf>,
    now_ms: u64,
}

fn describe_project(
    project_id: String,
    directory: PathBuf,
    facts: Result<WorktreeFactsResponse, String>,
    host: &HostNow<'_>,
) -> Described {
    let (free_bytes, file_system_bytes) = host_facts::space(&directory);
    let mut described = Described {
        directory,
        wire: ProjectWorktrees {
            project_id,
            free_bytes,
            file_system_bytes,
            ..ProjectWorktrees::default()
        },
        targets: Vec::new(),
        target_of: Vec::new(),
    };
    let facts = match facts {
        Ok(facts) => facts,
        Err(reason) => {
            described.wire.unavailable_reason = reason;
            return described;
        }
    };
    described.wire.base_branch = facts.base_branch;
    described.wire.last_fetch_at_ms = facts.last_fetch_at_ms;
    let paths: Vec<&str> = facts
        .worktrees
        .iter()
        .map(|facts| facts.path.as_str())
        .collect();
    for worktree in &facts.worktrees {
        let in_worktree =
            |directory: &Path| owner(&paths, directory) == Some(worktree.path.as_str());
        let session_ids = host
            .sessions
            .iter()
            .filter(|session| in_worktree(&session.directory))
            .map(|session| session.id.clone())
            .collect();
        let busy = host
            .processes
            .iter()
            .any(|directory| in_worktree(directory));
        let (info, target) = describe(worktree, session_ids, busy, host.now_ms);
        described.target_of.push(target.map(|target| {
            described.targets.push(target);
            described.targets.len() - 1
        }));
        described.wire.worktrees.push(info);
    }
    described
}

/// The worktree of `paths` that `directory` belongs to: the one with the
/// longest directory that contains it.
fn owner<'a>(paths: &[&'a str], directory: &Path) -> Option<&'a str> {
    paths
        .iter()
        .copied()
        .filter(|path| directory.starts_with(path))
        .max_by_key(|path| path.len())
}

/// The wire description of one worktree before its size, and the walk
/// target of its directory.
fn describe(
    facts: &WorktreeFacts,
    session_ids: Vec<String>,
    busy: bool,
    now_ms: u64,
) -> (WorktreeInfo, Option<WalkTarget>) {
    let root = PathBuf::from(&facts.path);
    let mut build_outputs = Vec::new();
    let mut user_paths = Vec::new();
    for ignored in &facts.ignored_paths {
        match build_output::rule_of(&root, ignored) {
            Some(rule) => build_outputs.push((ignored.trim_end_matches('/').to_owned(), rule)),
            None => user_paths.push(ignored.clone()),
        }
    }
    let host_flags = [
        (busy, WorktreeFlag::Busy),
        (
            !facts.is_missing && host_facts::is_on_other_file_system(&root),
            WorktreeFlag::OtherFileSystem,
        ),
        // Paths after the limit of the extension can be files of the user.
        (
            !user_paths.is_empty() || facts.ignored_paths_truncated,
            WorktreeFlag::IgnoredUserFiles,
        ),
    ];
    let view = HostView {
        has_session: !session_ids.is_empty(),
        flags: host_flags
            .into_iter()
            .filter_map(|(set, flag)| set.then_some(flag))
            .collect(),
    };
    let judgement = judge(facts, &view, now_ms);
    user_paths.truncate(MAX_USER_PATHS);
    let info = WorktreeInfo {
        path: facts.path.clone(),
        branch: facts.branch.clone(),
        head: facts.head.clone(),
        is_main: facts.is_main,
        is_bare: facts.is_bare,
        state: judgement.state.into(),
        flags: judgement.flags.into_iter().map(Into::into).collect(),
        session_ids,
        size: None,
        last_activity_at_ms: facts.last_activity_at_ms,
        merged_by: facts.merged_by,
        unpushed_commits: facts.unpushed_commits,
        refusals: judgement.refusals.into_iter().map(Into::into).collect(),
        ignored_user_paths: user_paths,
        locked_reason: facts.locked_reason.clone(),
        facts_unavailable_reason: facts.error.clone(),
    };
    let measurable = !facts.is_bare && !facts.is_missing;
    (
        info,
        measurable.then_some(WalkTarget {
            root,
            build_outputs,
        }),
    )
}
