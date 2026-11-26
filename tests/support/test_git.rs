//! The git extension of the end-to-end tests: it has the identifier of the
//! real git extension, `nomux.git`, its method `worktree_facts` and the four
//! methods of a clean.
//!
//! - `worktree_facts` does not read a repository: it gives the facts that a
//!   test wrote before, as an encoded `WorktreeFactsResponse` in the file
//!   `GIT_FACTS_FILE` of the home directory.
//! - The methods of a clean run the real `git` without the checks of the
//!   real extension: a test of the daemon shows what the daemon refuses
//!   itself. Each call is a line in `GIT_CHANGES_FILE`.
//!
//! The facts of a real repository and the checks of the real methods are
//! tested in the `extensions` repository.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use nomux_end_to_end::{GIT_CHANGES_FILE, GIT_FACTS_FILE};
use nomux_extension::{Call, CallError, ErrorCode, Extension, RunError};
use nomux_proto::wire::v1::{
    WorktreeDeleteRequest, WorktreeDeleteResponse, WorktreeFactsRequest, WorktreeFactsResponse,
    WorktreeMoveFromTrashRequest, WorktreeMoveFromTrashResponse, WorktreeMoveToTrashRequest,
    WorktreeMoveToTrashResponse, WorktreePruneRequest, WorktreePruneResponse,
};
use prost::Message;

fn main() -> Result<(), RunError> {
    Extension::new("nomux.git", env!("CARGO_PKG_VERSION"))
        .method("worktree_facts", facts)
        .method("worktree_move_to_trash", move_to_trash)
        .method("worktree_move_from_trash", move_from_trash)
        .method("worktree_delete", delete)
        .method("worktree_prune", prune)
        .run()
}

fn failed(message: String) -> CallError {
    CallError::new(ErrorCode::Failed, message)
}

fn home() -> Result<PathBuf, CallError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| failed("HOME is not set".to_owned()))
}

fn facts(_: &Call, _: WorktreeFactsRequest) -> Result<WorktreeFactsResponse, CallError> {
    let bytes = fs::read(home()?.join(GIT_FACTS_FILE))
        .map_err(|error| failed(format!("no facts: {error}")))?;
    WorktreeFactsResponse::decode(bytes.as_slice())
        .map_err(|error| failed(format!("the facts are not valid: {error}")))
}

/// Runs `git` in `repository` and writes the call to the log.
fn git(repository: &str, arguments: &[&str]) -> Result<(), CallError> {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home()?.join(GIT_CHANGES_FILE))
        .map_err(|error| failed(error.to_string()))?;
    writeln!(log, "{}", arguments.join(" ")).map_err(|error| failed(error.to_string()))?;
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .map_err(|error| failed(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failed(String::from_utf8_lossy(&output.stderr).into_owned()))
    }
}

fn move_to_trash(
    _: &Call,
    WorktreeMoveToTrashRequest {
        repository,
        path,
        trash_path,
        ..
    }: WorktreeMoveToTrashRequest,
) -> Result<WorktreeMoveToTrashResponse, CallError> {
    git(&repository, &["worktree", "move", &path, &trash_path])?;
    git(
        &repository,
        &["worktree", "lock", "--reason", "nomux-trash", &trash_path],
    )?;
    Ok(WorktreeMoveToTrashResponse::default())
}

fn move_from_trash(
    _: &Call,
    WorktreeMoveFromTrashRequest {
        repository,
        trash_path,
        path,
    }: WorktreeMoveFromTrashRequest,
) -> Result<WorktreeMoveFromTrashResponse, CallError> {
    git(&repository, &["worktree", "unlock", &trash_path])?;
    git(&repository, &["worktree", "move", &trash_path, &path])?;
    Ok(WorktreeMoveFromTrashResponse {})
}

fn delete(
    _: &Call,
    WorktreeDeleteRequest {
        repository,
        path,
        delete_branch,
        ..
    }: WorktreeDeleteRequest,
) -> Result<WorktreeDeleteResponse, CallError> {
    let repository = &repository;
    let branch = Command::new("git")
        .current_dir(&path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .map_err(|error| failed(error.to_string()))?;
    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_owned();
    // A worktree without the lock of the trash place: `unlock` fails, and
    // the removal follows.
    let _ = git(repository, &["worktree", "unlock", &path]);
    git(repository, &["worktree", "remove", &path])?;
    let mut response = WorktreeDeleteResponse::default();
    if delete_branch {
        git(repository, &["branch", "-D", &branch])?;
        response.branch_deleted = true;
    }
    Ok(response)
}

fn prune(_: &Call, request: WorktreePruneRequest) -> Result<WorktreePruneResponse, CallError> {
    git(&request.repository, &["worktree", "prune"])?;
    Ok(WorktreePruneResponse {
        pruned: request.paths,
        refusal: String::new(),
    })
}
