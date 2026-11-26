//! The worktrees of the Projects of the host: their state, their flags,
//! their size, the reasons why a removal of each one is refused, and the
//! clean that removes build output and moves finished worktrees to the trash
//! place. `spec/worktree.md` of the `proto` repository has the rules, and
//! `docs/worktrees.md` describes this part.
//!
//! - `facts`, `git_changes`: the git facts and the changes of git, through
//!   the git extension.
//! - `build_output`: the rules that decide which ignored directory is build
//!   output.
//! - `state`: the state, the flags and the refusals.
//! - `host_facts`, `tools`: the processes, the devices, the free space, and
//!   the tool that made a worktree.
//! - `sizes`, `size_walk`: the sizes in memory and the walk that measures
//!   them.
//! - `plan`, `clean`, `deletion`: the items of a clean and their run.
//! - `store`, `trash`: the marks and the trash place on disk.
//! - `disk`, `background`: the levels of the free space, the end of the
//!   grace time and the rules of the settings.

mod background;
mod build_output;
mod clean;
mod deletion;
mod disk;
mod facts;
mod git_changes;
mod host_facts;
mod plan;
mod size_walk;
mod sizes;
mod state;
mod store;
mod tools;
mod trash;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use nomux_ext_host::ExtensionHost;
use nomux_proto::wire::v1::{
    Error, ErrorCode, ProjectWorktrees, WorktreeChange, WorktreeChanged, WorktreeFacts,
    WorktreeFactsResponse, WorktreeFlag, WorktreeInfo, WorktreeListRequest, WorktreeListResponse,
    WorktreeMark, WorktreeMarkRequest, WorktreeMarkResponse, WorktreeState, event,
};
use tokio::sync::Notify;
use tokio::task::spawn_blocking;

pub(crate) use self::background::run as run_background;
use self::build_output::BuildOutputRules;
use self::plan::StoredPlans;
use self::size_walk::WalkTarget;
use self::sizes::{ProjectWalk, SizeCache};
use self::state::{HostView, judge};
use self::store::Store;
use crate::clock::now;
use crate::events::EventLog;
use crate::session_registry::SessionState;
use crate::sessions::Sessions;
use crate::settings::WorktreeSettings;
use crate::workspace::Workspace;

/// The largest number of ignored paths of the user in one worktree of a
/// response.
const MAX_USER_PATHS: usize = 32;
/// The reason of the lock of a worktree in the trash place.
const TRASH_LOCK_REASON: &str = "nomux-trash";

pub(crate) struct Worktrees {
    settings: WorktreeSettings,
    rules: Arc<BuildOutputRules>,
    sizes: Arc<SizeCache>,
    store: Store,
    plans: StoredPlans,
    /// One clean at a time.
    cleaning: tokio::sync::Mutex<()>,
    events: Arc<EventLog>,
    /// The level of the free space of each file system, by its device.
    disk_levels: Mutex<HashMap<u64, i32>>,
    /// The bytes that a clean of finished worktrees could remove on each
    /// file system, from the last list: all, and the build output.
    reclaimable: Mutex<HashMap<u64, (u64, u64)>>,
    /// The trash place changed: the timer of the grace time starts again.
    trash_changed: Notify,
    /// The rules of the settings should run.
    auto_wanted: Notify,
    /// A client subscribed to the events.
    subscribed: Notify,
}

/// What the worktrees need of the daemon.
pub(crate) struct Sources<'a> {
    pub(crate) workspace: &'a Workspace,
    pub(crate) sessions: &'a Sessions,
    pub(crate) extensions: &'a ExtensionHost,
}

/// One session that is not ended, with its working directory.
struct LiveSession {
    id: String,
    directory: PathBuf,
    /// `false` for a dormant session: its command does not run.
    has_process: bool,
}

/// When the sizes of a snapshot are measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sizes {
    /// Before the snapshot is complete.
    Wait,
    /// After the response.
    Later,
    /// Not now: the snapshot of a clean.
    Cached,
}

/// The Projects of a snapshot.
struct DescribedProjects {
    projects: Vec<Described>,
    live_sessions: HashSet<String>,
}

/// One Project of a snapshot.
struct Described {
    directory: PathBuf,
    wire: ProjectWorktrees,
    /// The worktrees to measure: each one whose directory exists.
    targets: Vec<WalkTarget>,
    /// For each worktree of `wire`, its index in `targets`.
    target_of: Vec<Option<usize>>,
    /// For each worktree of `wire`, what a plan needs and the wire does not
    /// have.
    details: Vec<Detail>,
}

/// What a plan needs of one worktree.
#[derive(Debug, Clone, Default)]
struct Detail {
    /// The build output directories, relative, with their rule.
    build_outputs: Vec<(String, String)>,
    /// Each ignored path that is not build output.
    user_paths: Vec<String>,
    /// Sessions with a process in the worktree.
    live_sessions: usize,
    /// Dormant sessions in the worktree.
    dormant_sessions: usize,
    /// The state that the worktree has without its sessions.
    state_without_sessions: WorktreeState,
}

/// What the rules need of the daemon, for the thread that reads the host.
struct Judging {
    rules: Arc<BuildOutputRules>,
    marks: HashMap<PathBuf, WorktreeMark>,
    /// The identifier of each trash entry, by its path in the trash place.
    trash_ids: HashMap<PathBuf, String>,
    stale_after: Duration,
}

impl Worktrees {
    /// The worktrees with the file of the state directory `state_directory`.
    pub(crate) fn new(
        state_directory: &Path,
        settings: WorktreeSettings,
        events: Arc<EventLog>,
    ) -> Self {
        Self {
            rules: Arc::new(BuildOutputRules::new(settings.build_output_rules.clone())),
            sizes: Arc::new(SizeCache::new(settings.size_ttl)),
            store: Store::open(state_directory),
            plans: StoredPlans::default(),
            cleaning: tokio::sync::Mutex::default(),
            events,
            disk_levels: Mutex::default(),
            reclaimable: Mutex::default(),
            trash_changed: Notify::new(),
            auto_wanted: Notify::new(),
            subscribed: Notify::new(),
            settings,
        }
    }

    /// `Worktree.List`.
    pub(crate) async fn list(
        &self,
        request: &WorktreeListRequest,
        sources: Sources<'_>,
    ) -> Result<WorktreeListResponse, Error> {
        let sizes = if request.wait_for_sizes {
            Sizes::Wait
        } else {
            Sizes::Later
        };
        let projects = self.snapshot(&request.project_id, &sources, sizes).await?;
        self.remember(&projects);
        let directories: Vec<(String, PathBuf)> = projects
            .iter()
            .map(|project| (project.wire.project_id.clone(), project.directory.clone()))
            .collect();
        disk::check(self, &directories);
        if !self.settings.auto.is_empty() {
            self.auto_wanted.notify_one();
        }
        Ok(WorktreeListResponse {
            projects: projects.into_iter().map(|project| project.wire).collect(),
        })
    }

    /// `Worktree.Mark`.
    pub(crate) async fn mark(
        &self,
        request: &WorktreeMarkRequest,
        sources: Sources<'_>,
    ) -> Result<WorktreeMarkResponse, Error> {
        let projects = self.snapshot("", &sources, Sizes::Cached).await?;
        let Some((project, worktree)) = find_worktree(&projects, &request.path) else {
            return Err(not_found(format!(
                "{} is no worktree of a Project",
                request.path
            )));
        };
        let path = PathBuf::from(&worktree.path);
        self.store
            .set_mark(&path, request.mark())
            .map_err(|error| failed(format!("the mark was not saved: {error}")))?;
        self.publish(WorktreeChanged {
            project_id: project.wire.project_id.clone(),
            path: worktree.path.clone(),
            change: WorktreeChange::Marked.into(),
            branch: worktree.branch.clone(),
            ..WorktreeChanged::default()
        });
        Ok(WorktreeMarkResponse {})
    }

    /// A client subscribed to the events: the free space is read every 5
    /// minutes while a subscription exists.
    pub(crate) fn on_subscribe(&self) {
        self.subscribed.notify_one();
    }

    fn publish(&self, changed: WorktreeChanged) {
        self.events.publish(event::Body::WorktreeChanged(changed));
    }

    /// The facts, the state, the flags, the refusals and the sizes of the
    /// worktrees of one Project, or of each Project with an empty
    /// `project_id`.
    async fn snapshot(
        &self,
        project_id: &str,
        sources: &Sources<'_>,
        sizes: Sizes,
    ) -> Result<Vec<Described>, Error> {
        let projects = sources.workspace.projects(project_id)?;
        let sessions = live_sessions(sources.sessions);
        let mut listed = Vec::new();
        for (project_id, directory) in projects {
            let facts =
                facts::of_project(sources.extensions, &directory, &self.settings.base).await;
            listed.push((project_id, directory, facts));
        }
        let judging = Judging {
            rules: Arc::clone(&self.rules),
            marks: self.store.marks(),
            trash_ids: self
                .store
                .trash_entries()
                .into_iter()
                .map(|entry| (entry.trash_path, entry.id))
                .collect(),
            stale_after: self.settings.stale_after,
        };
        let mut described = spawn_blocking(move || describe_all(listed, &sessions, &judging))
            .await
            .map_err(|error| failed(format!("the worktrees were not read: {error}")))?;
        let live: HashSet<String> = std::mem::take(&mut described.live_sessions);
        let mut walks = Vec::new();
        for project in &mut described.projects {
            walks.extend(self.add_sizes(project, &live));
        }
        match sizes {
            Sizes::Wait if !walks.is_empty() => {
                for walk in walks {
                    self.sizes.measure(walk).await;
                }
                for project in &mut described.projects {
                    self.add_sizes(project, &live);
                }
            }
            Sizes::Later => {
                for walk in walks {
                    self.sizes.measure_later(walk);
                }
            }
            Sizes::Wait | Sizes::Cached => {}
        }
        Ok(described.projects)
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

    /// Keeps the bytes that a clean could remove on each file system of
    /// `projects`, for the levels of the free space.
    fn remember(&self, projects: &[Described]) {
        let mut reclaimable: HashMap<u64, (u64, u64)> = HashMap::new();
        for project in projects {
            let Some(device) = host_facts::device(&project.directory) else {
                continue;
            };
            let sum = reclaimable.entry(device).or_default();
            for worktree in &project.wire.worktrees {
                if let (true, Some(size)) = (worktree.refusals.is_empty(), &worktree.size) {
                    sum.0 += size.total_bytes;
                    sum.1 += size.build_output_bytes;
                }
            }
        }
        lock(&self.reclaimable).extend(reclaimable);
    }
}

fn find_worktree<'a>(
    projects: &'a [Described],
    path: &str,
) -> Option<(&'a Described, &'a WorktreeInfo)> {
    let real = Path::new(path).canonicalize().ok();
    projects.iter().find_map(|project| {
        project
            .wire
            .worktrees
            .iter()
            .find(|worktree| {
                worktree.path == path || real.as_deref() == Some(Path::new(&worktree.path))
            })
            .map(|worktree| (project, worktree))
    })
}

fn live_sessions(sessions: &Sessions) -> Vec<LiveSession> {
    sessions
        .list(false)
        .into_iter()
        .filter_map(|session| {
            let has_process = !matches!(session.record.state, SessionState::Dormant);
            let directory = session
                .resolved_working_directory
                .or_else(|| session.metadata.map(|metadata| metadata.working_directory))?;
            Some(LiveSession {
                id: session.record.id.to_string(),
                directory,
                has_process,
            })
        })
        .collect()
}

/// Reads the host for each Project and applies the rules. Runs on a thread
/// that can wait: it reads the process table and the worktrees.
fn describe_all(
    listed: Vec<(String, PathBuf, Result<WorktreeFactsResponse, String>)>,
    sessions: &[LiveSession],
    judging: &Judging,
) -> DescribedProjects {
    let host = HostNow {
        sessions,
        processes: host_facts::process_directories(),
        now_ms: now().0,
    };
    DescribedProjects {
        projects: listed
            .into_iter()
            .map(|(id, directory, facts)| describe_project(id, directory, facts, &host, judging))
            .collect(),
        live_sessions: sessions.iter().map(|session| session.id.clone()).collect(),
    }
}

/// What a snapshot reads of the host one time for all Projects.
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
    judging: &Judging,
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
        details: Vec::new(),
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
        let sessions: Vec<&LiveSession> = host
            .sessions
            .iter()
            .filter(|session| in_worktree(&session.directory))
            .collect();
        let busy = host
            .processes
            .iter()
            .any(|directory| in_worktree(directory));
        let (info, detail, target) = describe(worktree, &sessions, busy, host.now_ms, judging);
        described.target_of.push(target.map(|target| {
            described.targets.push(target);
            described.targets.len() - 1
        }));
        described.wire.worktrees.push(info);
        described.details.push(detail);
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

/// The wire description of one worktree before its size, what a plan needs
/// of it, and the walk target of its directory.
fn describe(
    facts: &WorktreeFacts,
    sessions: &[&LiveSession],
    busy: bool,
    now_ms: u64,
    judging: &Judging,
) -> (WorktreeInfo, Detail, Option<WalkTarget>) {
    let root = PathBuf::from(&facts.path);
    let mut build_outputs = Vec::new();
    let mut user_paths = Vec::new();
    for ignored in &facts.ignored_paths {
        match judging.rules.rule_of(&root, ignored) {
            Some(rule) => build_outputs.push((ignored.trim_end_matches('/').to_owned(), rule)),
            None => user_paths.push(ignored.clone()),
        }
    }
    let build_locked = build_outputs
        .iter()
        .any(|(path, _)| build_output::holds_build_lock(&root.join(path)));
    let in_trash = facts.is_locked && facts.locked_reason == TRASH_LOCK_REASON;
    let host_flags = [
        (busy || build_locked, WorktreeFlag::Busy),
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
    let mark = judging
        .marks
        .get(&root)
        .copied()
        .unwrap_or(WorktreeMark::Unspecified);
    let view = HostView {
        has_session: !sessions.is_empty(),
        flags: host_flags
            .into_iter()
            .filter_map(|(set, flag)| set.then_some(flag))
            .collect(),
        mark,
        in_trash,
        no_trash_place: !facts.is_main
            && !facts.is_missing
            && !in_trash
            && !host_facts::has_writable_parent(&root),
    };
    let judgement = judge(facts, &view, now_ms, judging.stale_after);
    let without_sessions = judge(
        facts,
        &HostView {
            has_session: false,
            ..view.clone()
        },
        now_ms,
        judging.stale_after,
    );
    let live_sessions = sessions
        .iter()
        .filter(|session| session.has_process)
        .count();
    let mut wire_user_paths = user_paths.clone();
    wire_user_paths.truncate(MAX_USER_PATHS);
    let info = WorktreeInfo {
        path: facts.path.clone(),
        branch: facts.branch.clone(),
        head: facts.head.clone(),
        is_main: facts.is_main,
        is_bare: facts.is_bare,
        state: judgement.state.into(),
        flags: judgement.flags.into_iter().map(Into::into).collect(),
        session_ids: sessions.iter().map(|session| session.id.clone()).collect(),
        size: None,
        last_activity_at_ms: facts.last_activity_at_ms,
        merged_by: facts.merged_by,
        unpushed_commits: facts.unpushed_commits,
        refusals: judgement.refusals.into_iter().map(Into::into).collect(),
        ignored_user_paths: wire_user_paths,
        locked_reason: facts.locked_reason.clone(),
        facts_unavailable_reason: facts.error.clone(),
        tool: tools::tool_of(&facts.path).to_owned(),
        mark: mark.into(),
        trash_id: trash_id(&root, in_trash, judging),
    };
    let detail = Detail {
        build_outputs: build_outputs.clone(),
        user_paths,
        live_sessions,
        dormant_sessions: sessions.len() - live_sessions,
        state_without_sessions: without_sessions.state,
    };
    let measurable = !facts.is_bare && !facts.is_missing;
    (
        info,
        detail,
        measurable.then_some(WalkTarget {
            root,
            build_outputs,
        }),
    )
}

/// The identifier of the trash entry of the worktree `root`: from the store,
/// or the name of its directory for an entry that the store does not have.
fn trash_id(root: &Path, in_trash: bool, judging: &Judging) -> String {
    if !in_trash {
        return String::new();
    }
    judging
        .trash_ids
        .get(root)
        .cloned()
        .unwrap_or_else(|| last_part(root))
}

/// The last part of `path`, or an empty text.
fn last_part(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // No code panics while it holds a lock of this module, thus the value
    // is whole.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn failed(message: String) -> Error {
    Error {
        code: ErrorCode::Failed.into(),
        message,
    }
}

fn not_found(message: String) -> Error {
    Error {
        code: ErrorCode::NotFound.into(),
        message,
    }
}
