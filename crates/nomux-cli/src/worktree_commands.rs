//! The `worktree` commands: the worktrees of the Projects of a host.
//! `docs/worktrees.md` has the model.

mod arguments;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nomux_client::{ClientError, ProjectWorktrees, Workspace};

pub(crate) use arguments::WorktreeCommand;

use crate::commands::KIND;
use crate::output::Format;
use crate::report::Report;
use crate::selected_host;
use crate::workspace_commands::node_path::find;

/// The result of `worktree ls`.
#[derive(Debug)]
pub(crate) struct WorktreeListing {
    /// The workspace tree, for the names of the Projects.
    pub(crate) workspace: Workspace,
    pub(crate) projects: Vec<ProjectWorktrees>,
}

/// `worktree ls`.
pub(crate) async fn run(
    command: WorktreeCommand,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let WorktreeCommand::Ls { project, all, wait } = command;
    let host = selected_host::connect(host_name, format).await?;
    let mut client = host.client(KIND).await?;
    let workspace = client.workspace().await?;
    let project_id = match (project, all) {
        (_, true) => None,
        (Some(project), false) => Some(find(&workspace.tree, &project)?.id.clone()),
        (None, false) => Some(current_project(&workspace, host_name.is_some())?),
    };
    let projects = client.worktrees(project_id.as_deref(), wait).await?;
    Ok(Report::Worktrees(WorktreeListing {
        workspace,
        projects,
    }))
}

/// The Project of the current directory, on the local host.
fn current_project(workspace: &Workspace, on_remote_host: bool) -> Result<String, ClientError> {
    let no_project = |reason: &str| {
        ClientError::Failed(format!(
            "{reason}: name a Project, or give --all for each Project"
        ))
    };
    if on_remote_host {
        return Err(no_project(
            "with --host, the current directory has no Project",
        ));
    }
    let directory = env::current_dir()
        .and_then(|directory| directory.canonicalize())
        .map_err(|error| no_project(&format!("the current directory is not valid: {error}")))?;
    let tree = &workspace.tree;
    tree.project_of(&directory)
        .or_else(|| tree.project_of(&main_worktree(&directory)?))
        .map(|project| project.id.clone())
        .ok_or_else(|| no_project("the current directory is in no Project"))
}

/// The main worktree of the git repository of `directory`: the directory
/// of its common git directory. A linked worktree can be outside the
/// directory of its Project. `None` outside a repository and without git.
fn main_worktree(directory: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let common = PathBuf::from(
        String::from_utf8(output.stdout)
            .ok()?
            .trim_end_matches('\n'),
    );
    common.parent()?.canonicalize().ok()
}
