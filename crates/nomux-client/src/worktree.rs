//! The worktrees of the Projects of a host, as `Worktree.List` gives them:
//! their state, their flags, their size and the reasons why a removal of
//! each one is refused.

use nomux_proto::wire::v1::{self as wire, WorktreeListRequest, request, response};

use crate::client::unexpected_result;
use crate::{Client, ClientError};

/// The worktrees of one Project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectWorktrees {
    /// The identifier of the Project.
    pub project_id: String,
    /// Why the daemon has no worktree of the Project, and what the user can
    /// do. `None` when it has them.
    pub unavailable_reason: Option<String>,
    /// The base branch of the merge rules, for example `origin/main`.
    pub base_branch: Option<String>,
    /// The time of the last fetch: milliseconds after the Unix epoch.
    pub last_fetch_at_ms: Option<u64>,
    /// The free bytes of the file system of the Project directory.
    pub free_bytes: Option<u64>,
    /// The size of that file system in bytes.
    pub file_system_bytes: Option<u64>,
    /// The main worktree first, then the linked worktrees.
    pub worktrees: Vec<Worktree>,
}

/// One worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// The absolute path of its directory.
    pub path: String,
    /// The short name of its branch. `None` for a detached `HEAD`.
    pub branch: Option<String>,
    /// The commit of `HEAD`.
    pub head: Option<String>,
    /// `true` for the main worktree of the repository.
    pub is_main: bool,
    /// `true` for the entry of a bare repository.
    pub is_bare: bool,
    /// The state.
    pub state: WorktreeState,
    /// The flags, in the sequence of the protocol.
    pub flags: Vec<WorktreeFlag>,
    /// The sessions that are not ended in the worktree.
    pub session_ids: Vec<String>,
    /// `None` when the daemon did not measure the worktree yet.
    pub size: Option<WorktreeSize>,
    /// The last activity: milliseconds after the Unix epoch.
    pub last_activity_at_ms: Option<u64>,
    /// The rule that found the branch merged.
    pub merged_by: MergedBy,
    /// The commits that are not pushed. The count stops at 1000.
    pub unpushed_commits: u32,
    /// Why a removal of the directory is refused, the most important reason
    /// first. Empty when a removal loses no work.
    pub refusals: Vec<Refusal>,
    /// The ignored paths that are not build output, 32 at most.
    pub ignored_user_paths: Vec<String>,
    /// The reason of the lock.
    pub locked_reason: Option<String>,
    /// Why the daemon does not know the git facts of the worktree.
    pub facts_unavailable_reason: Option<String>,
}

/// The size of a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSize {
    /// The allocated bytes of the worktree.
    pub total_bytes: u64,
    /// The part of `total_bytes` in build output.
    pub build_output_bytes: u64,
    /// When the daemon measured it: milliseconds after the Unix epoch.
    pub measured_at_ms: u64,
    /// The size is old: the daemon measures it again.
    pub is_old: bool,
    /// Each build output directory, the largest first.
    pub build_outputs: Vec<BuildOutput>,
}

/// One build output directory of a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    /// The path relative to the worktree.
    pub path: String,
    /// The identifier of the rule, for example `cargo-target`.
    pub rule: String,
    /// The allocated bytes below the directory.
    pub bytes: u64,
}

/// The state of a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeState {
    /// A session that is not ended is in the worktree.
    Active,
    /// The branch is merged into the base branch.
    Finished,
    /// No activity for the stale time.
    Stale,
    /// Each other worktree.
    Idle,
    /// A state from a later protocol version.
    Unknown,
}

/// A fact of a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeFlag {
    /// Changes that are not committed, or an operation in progress.
    Dirty,
    /// Commits that are not pushed.
    Unpushed,
    /// A detached `HEAD` with commits that no branch has.
    DetachedCommits,
    /// The worktree is locked.
    Locked,
    /// The worktree has a `.gitmodules` file.
    Submodule,
    /// Ignored paths that are not build output.
    IgnoredUserFiles,
    /// A process has its current directory in the worktree.
    Busy,
    /// The directory does not exist.
    Missing,
    /// The directory is on a different file system than its parent.
    OtherFileSystem,
    /// The remote-tracking branch of the upstream branch is gone.
    UpstreamGone,
    /// A flag from a later protocol version.
    Unknown,
}

/// The rule that found the branch merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergedBy {
    /// Not merged, or not known.
    None,
    /// The base branch has the commit of `HEAD`.
    Ancestor,
    /// A merge into the base branch changes nothing.
    Tree,
    /// The upstream branch is gone.
    UpstreamGone,
    /// A rule from a later protocol version.
    Unknown,
}

/// Why a removal of the directory of a worktree is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The main worktree.
    MainWorktree,
    /// The directory does not exist.
    DirectoryMissing,
    /// The git facts are not known.
    FactsUnknown,
    /// A session is in the worktree.
    ActiveSession,
    /// A process is in the worktree.
    ProcessRunning,
    /// Changes that are not committed.
    UncommittedChanges,
    /// Commits that are not pushed.
    UnpushedCommits,
    /// Commits that no branch has.
    DetachedCommits,
    /// The worktree is locked.
    Locked,
    /// The worktree has a submodule.
    Submodule,
    /// Ignored files that are not build output.
    IgnoredUserFiles,
    /// The worktree is on a different file system.
    OtherFileSystem,
    /// The worktree is not finished.
    NotFinished,
    /// A reason from a later protocol version.
    Unknown,
}

impl Client {
    /// The worktrees of the Project `project_id`, or of each Project. With
    /// `wait_for_sizes`, the daemon measures each size that is absent or old
    /// before it answers; else it answers with the sizes that it has and
    /// measures after the answer.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier, and
    /// [`ClientError::Failed`] if the node is a Group or if the daemon is
    /// older than this request.
    pub async fn worktrees(
        &mut self,
        project_id: Option<&str>,
        wait_for_sizes: bool,
    ) -> Result<Vec<ProjectWorktrees>, ClientError> {
        let request = WorktreeListRequest {
            project_id: project_id.unwrap_or_default().to_owned(),
            wait_for_sizes,
        };
        match self.call(request::Body::WorktreeList(request)).await? {
            response::Result::WorktreeList(listed) => Ok(listed
                .projects
                .into_iter()
                .map(ProjectWorktrees::from)
                .collect()),
            _ => Err(unexpected_result()),
        }
    }
}

impl From<wire::ProjectWorktrees> for ProjectWorktrees {
    fn from(project: wire::ProjectWorktrees) -> Self {
        Self {
            project_id: project.project_id,
            unavailable_reason: text(project.unavailable_reason),
            base_branch: text(project.base_branch),
            last_fetch_at_ms: number(project.last_fetch_at_ms),
            free_bytes: number(project.free_bytes),
            file_system_bytes: number(project.file_system_bytes),
            worktrees: project.worktrees.into_iter().map(Worktree::from).collect(),
        }
    }
}

impl From<wire::WorktreeInfo> for Worktree {
    fn from(worktree: wire::WorktreeInfo) -> Self {
        Self {
            state: state(worktree.state),
            flags: worktree.flags.iter().map(|value| flag(*value)).collect(),
            merged_by: merged_by(worktree.merged_by),
            refusals: worktree
                .refusals
                .iter()
                .map(|value| refusal(*value))
                .collect(),
            path: worktree.path,
            branch: text(worktree.branch),
            head: text(worktree.head),
            is_main: worktree.is_main,
            is_bare: worktree.is_bare,
            session_ids: worktree.session_ids,
            size: worktree.size.map(WorktreeSize::from),
            last_activity_at_ms: number(worktree.last_activity_at_ms),
            unpushed_commits: worktree.unpushed_commits,
            ignored_user_paths: worktree.ignored_user_paths,
            locked_reason: text(worktree.locked_reason),
            facts_unavailable_reason: text(worktree.facts_unavailable_reason),
        }
    }
}

impl From<wire::WorktreeSize> for WorktreeSize {
    fn from(size: wire::WorktreeSize) -> Self {
        Self {
            total_bytes: size.total_bytes,
            build_output_bytes: size.build_output_bytes,
            measured_at_ms: size.measured_at_ms,
            is_old: size.is_old,
            build_outputs: size
                .build_outputs
                .into_iter()
                .map(|output| BuildOutput {
                    path: output.path,
                    rule: output.rule,
                    bytes: output.bytes,
                })
                .collect(),
        }
    }
}

fn text(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn number(value: u64) -> Option<u64> {
    (value != 0).then_some(value)
}

fn state(value: i32) -> WorktreeState {
    match wire::WorktreeState::try_from(value) {
        Ok(wire::WorktreeState::Active) => WorktreeState::Active,
        Ok(wire::WorktreeState::Finished) => WorktreeState::Finished,
        Ok(wire::WorktreeState::Stale) => WorktreeState::Stale,
        Ok(wire::WorktreeState::Idle) => WorktreeState::Idle,
        Ok(wire::WorktreeState::Unspecified) | Err(_) => WorktreeState::Unknown,
    }
}

fn flag(value: i32) -> WorktreeFlag {
    match wire::WorktreeFlag::try_from(value) {
        Ok(wire::WorktreeFlag::Dirty) => WorktreeFlag::Dirty,
        Ok(wire::WorktreeFlag::Unpushed) => WorktreeFlag::Unpushed,
        Ok(wire::WorktreeFlag::DetachedCommits) => WorktreeFlag::DetachedCommits,
        Ok(wire::WorktreeFlag::Locked) => WorktreeFlag::Locked,
        Ok(wire::WorktreeFlag::Submodule) => WorktreeFlag::Submodule,
        Ok(wire::WorktreeFlag::IgnoredUserFiles) => WorktreeFlag::IgnoredUserFiles,
        Ok(wire::WorktreeFlag::Busy) => WorktreeFlag::Busy,
        Ok(wire::WorktreeFlag::Missing) => WorktreeFlag::Missing,
        Ok(wire::WorktreeFlag::OtherFileSystem) => WorktreeFlag::OtherFileSystem,
        Ok(wire::WorktreeFlag::UpstreamGone) => WorktreeFlag::UpstreamGone,
        Ok(wire::WorktreeFlag::Unspecified) | Err(_) => WorktreeFlag::Unknown,
    }
}

fn merged_by(value: i32) -> MergedBy {
    match wire::WorktreeMergedBy::try_from(value) {
        Ok(wire::WorktreeMergedBy::Unspecified) => MergedBy::None,
        Ok(wire::WorktreeMergedBy::Ancestor) => MergedBy::Ancestor,
        Ok(wire::WorktreeMergedBy::Tree) => MergedBy::Tree,
        Ok(wire::WorktreeMergedBy::UpstreamGone) => MergedBy::UpstreamGone,
        Err(_) => MergedBy::Unknown,
    }
}

fn refusal(value: i32) -> Refusal {
    match wire::WorktreeRefusal::try_from(value) {
        Ok(wire::WorktreeRefusal::MainWorktree) => Refusal::MainWorktree,
        Ok(wire::WorktreeRefusal::DirectoryMissing) => Refusal::DirectoryMissing,
        Ok(wire::WorktreeRefusal::FactsUnknown) => Refusal::FactsUnknown,
        Ok(wire::WorktreeRefusal::ActiveSession) => Refusal::ActiveSession,
        Ok(wire::WorktreeRefusal::ProcessRunning) => Refusal::ProcessRunning,
        Ok(wire::WorktreeRefusal::UncommittedChanges) => Refusal::UncommittedChanges,
        Ok(wire::WorktreeRefusal::UnpushedCommits) => Refusal::UnpushedCommits,
        Ok(wire::WorktreeRefusal::DetachedCommits) => Refusal::DetachedCommits,
        Ok(wire::WorktreeRefusal::Locked) => Refusal::Locked,
        Ok(wire::WorktreeRefusal::Submodule) => Refusal::Submodule,
        Ok(wire::WorktreeRefusal::IgnoredUserFiles) => Refusal::IgnoredUserFiles,
        Ok(wire::WorktreeRefusal::OtherFileSystem) => Refusal::OtherFileSystem,
        Ok(wire::WorktreeRefusal::NotFinished) => Refusal::NotFinished,
        Ok(wire::WorktreeRefusal::Unspecified) | Err(_) => Refusal::Unknown,
    }
}
