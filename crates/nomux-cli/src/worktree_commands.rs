//! The `worktree` commands: the worktrees of the Projects of a host, the
//! clean and the trash place. `docs/worktrees.md` has the model.

mod arguments;
mod clean;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use nomux_client::{
    Client, ClientError, Mark, Plan, ProjectWorktrees, TrashEntry, TrashRemoval, Workspace,
};

pub(crate) use arguments::WorktreeCommand;
use arguments::{MarkValue, ProjectSelection, TrashCommand};

use crate::commands::KIND;
use crate::exit_code;
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

/// The result of the other `worktree` commands.
#[derive(Debug)]
pub(crate) enum WorktreeReport {
    /// The plan of `clean` that did not run: a dry run, or a question
    /// without a yes.
    Plan(Plan),
    /// A clean ran.
    Cleaned {
        plan: Plan,
        cleaned: nomux_client::Cleaned,
    },
    Trash(Vec<TrashEntry>),
    Restored(TrashEntry),
    TrashEmptied(Vec<TrashRemoval>),
    Marked {
        path: String,
        mark: Mark,
    },
}

impl WorktreeReport {
    pub(crate) fn exit_code(&self) -> u8 {
        let all_done = match self {
            Self::Cleaned { cleaned, .. } => cleaned
                .results
                .iter()
                .all(|result| result.outcome == nomux_client::CleanOutcome::Done),
            Self::TrashEmptied(removals) => removals.iter().all(|removal| removal.removed),
            _ => true,
        };
        if all_done {
            exit_code::SUCCESS
        } else {
            exit_code::ERROR
        }
    }
}

/// A `worktree` command.
pub(crate) async fn run(
    command: WorktreeCommand,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let host = selected_host::connect(host_name, format).await?;
    let mut client = host.client(KIND).await?;
    let on_remote_host = host_name.is_some();
    match command {
        WorktreeCommand::Ls {
            projects,
            wait,
            fetch,
        } => {
            let workspace = client.workspace().await?;
            let project_id = selected_project(&workspace, &projects, on_remote_host)?;
            if fetch {
                fetch_projects(&workspace, project_id.as_deref(), on_remote_host)?;
            }
            let projects = client.worktrees(project_id.as_deref(), wait).await?;
            Ok(Report::Worktrees(WorktreeListing {
                workspace,
                projects,
            }))
        }
        WorktreeCommand::Clean(arguments) => {
            clean::run(&mut client, &arguments, on_remote_host).await
        }
        WorktreeCommand::Restore { name } => {
            let entry = client.restore_worktree(&name).await?;
            Ok(Report::Worktree(WorktreeReport::Restored(entry)))
        }
        WorktreeCommand::Trash {
            command: TrashCommand::Ls { project },
        } => {
            let project_id = match project {
                Some(project) => {
                    let workspace = client.workspace().await?;
                    Some(find(&workspace.tree, &project)?.id.clone())
                }
                None => None,
            };
            let entries = client.trash_entries(project_id.as_deref()).await?;
            Ok(Report::Worktree(WorktreeReport::Trash(entries)))
        }
        WorktreeCommand::Trash {
            command: TrashCommand::Empty { ids, yes },
        } => {
            if !yes {
                let entries = client.trash_entries(None).await?;
                let count = if ids.is_empty() {
                    entries.len()
                } else {
                    ids.len()
                };
                clean::confirm(&format!(
                    "Remove {count} worktree(s) of the trash place now? They cannot be restored \
                     after this"
                ))?;
            }
            let removals = client.empty_trash(&ids).await?;
            Ok(Report::Worktree(WorktreeReport::TrashEmptied(removals)))
        }
        WorktreeCommand::Mark { worktree, mark } => {
            let mark = match mark {
                MarkValue::Done => Mark::Finished,
                MarkValue::Keep => Mark::Keep,
            };
            mark_worktree(&mut client, &worktree, mark).await
        }
        WorktreeCommand::Unmark { worktree } => {
            mark_worktree(&mut client, &worktree, Mark::None).await
        }
    }
}

async fn mark_worktree(
    client: &mut Client,
    worktree: &str,
    mark: Mark,
) -> Result<Report, ClientError> {
    let path = worktree_path(client, worktree).await?;
    client.mark_worktree(&path, mark).await?;
    Ok(Report::Worktree(WorktreeReport::Marked { path, mark }))
}

/// The identifier of the Project of the selection: `None` for `--all`.
fn selected_project(
    workspace: &Workspace,
    selection: &ProjectSelection,
    on_remote_host: bool,
) -> Result<Option<String>, ClientError> {
    match (&selection.project, selection.all) {
        (_, true) => Ok(None),
        (Some(project), false) => Ok(Some(find(&workspace.tree, project)?.id.clone())),
        (None, false) => Ok(Some(current_project(workspace, on_remote_host)?)),
    }
}

/// The absolute path of the worktree that `worktree` names: a directory
/// that exists, or the name of the last part of the path of one worktree of
/// the Projects.
async fn worktree_path(client: &mut Client, worktree: &str) -> Result<String, ClientError> {
    if let Ok(path) = Path::new(worktree).canonicalize() {
        return Ok(path.display().to_string());
    }
    let projects = client.worktrees(None, false).await?;
    let mut named = projects
        .iter()
        .flat_map(|project| &project.worktrees)
        .filter(|listed| {
            Path::new(&listed.path)
                .file_name()
                .is_some_and(|name| name == worktree)
        });
    match (named.next(), named.next()) {
        (Some(listed), None) => Ok(listed.path.clone()),
        (Some(_), Some(_)) => Err(ClientError::Failed(format!(
            "more than one worktree has the name {worktree}: give its path"
        ))),
        (None, _) => Err(ClientError::NotFound(format!(
            "no worktree has the path or the name {worktree}"
        ))),
    }
}

/// Runs `git fetch --prune` of the user in the directory of each Project of
/// the selection. A fetch that fails is a warning: the list still shows the
/// facts that the repository has.
fn fetch_projects(
    workspace: &Workspace,
    project_id: Option<&str>,
    on_remote_host: bool,
) -> Result<(), ClientError> {
    if on_remote_host {
        return Err(ClientError::Failed(
            "--fetch runs git on this computer: it is not available with --host".to_owned(),
        ));
    }
    let directories: Vec<PathBuf> = workspace
        .tree
        .nodes()
        .into_iter()
        .filter(|node| project_id.is_none_or(|id| node.id == id))
        .filter_map(|node| node.directory().map(Path::to_path_buf))
        .collect();
    for directory in directories {
        let fetched = Command::new("git")
            .arg("-C")
            .arg(&directory)
            .args(["fetch", "--prune", "--quiet"])
            .stdin(Stdio::null())
            .status();
        match fetched {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!(
                "nomux: git fetch in {} ended with {status}",
                directory.display()
            ),
            Err(error) => eprintln!(
                "nomux: git fetch in {} did not start: {error}",
                directory.display()
            ),
        }
    }
    Ok(())
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
