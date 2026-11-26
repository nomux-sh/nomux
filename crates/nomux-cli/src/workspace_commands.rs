//! The `project` and `group` commands: the workspace tree of the Groups and
//! the Projects of a host. `docs/workspace.md` has the model.

mod arguments;
pub(crate) mod node_path;
mod report;

use std::env;
use std::path::{self, Path, PathBuf};
use std::process::{Command, Stdio};

use nomux_client::{ClientError, Node, WorkspaceTree};

pub(crate) use arguments::{GroupCommand, ProjectCommand};
pub(crate) use report::{Listing, TreeChange, WorkspaceReport};

use self::node_path::{find, path_of};
use crate::commands::KIND;
use crate::output::Format;
use crate::report::Report;
use crate::selected_host;

/// `project add`, `project rm` and `project ls`.
pub(crate) async fn project(
    command: ProjectCommand,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let host = selected_host::connect(host_name, format).await?;
    let mut client = host.client(KIND).await?;
    let workspace = client.workspace().await?;
    let tree = &workspace.tree;
    let report = match command {
        ProjectCommand::Add {
            directory,
            name,
            group,
        } => {
            let directory = project_directory(directory, host_name.is_some())?;
            let name = match name {
                Some(name) => name,
                None => last_part(&directory)?,
            };
            let parent = group.map(|group| find(tree, &group)).transpose()?;
            let parent_id = parent.map(|parent| parent.id.as_str());
            let (revision, node) = client
                .add_project(&directory, &name, parent_id, None)
                .await?;
            changed(TreeChange::ProjectAdded, tree, node, revision)
        }
        ProjectCommand::Rm { project } => {
            let node = find(tree, &project)?.clone();
            let revision = client.remove_project(&node.id).await?;
            changed(TreeChange::ProjectRemoved, tree, node, revision)
        }
        ProjectCommand::Ls => WorkspaceReport::Listed {
            workspace,
            listing: Listing::Projects,
        },
    };
    Ok(report.into())
}

/// `group new`, `group rename`, `group move`, `group rm` and `group ls`.
pub(crate) async fn group(
    command: GroupCommand,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let host = selected_host::connect(host_name, format).await?;
    let mut client = host.client(KIND).await?;
    let workspace = client.workspace().await?;
    let tree = &workspace.tree;
    let report = match command {
        GroupCommand::New { name, parent } => {
            let parent = parent.map(|parent| find(tree, &parent)).transpose()?;
            let parent_id = parent.map(|parent| parent.id.as_str());
            let (revision, node) = client.create_group(&name, parent_id, None).await?;
            changed(TreeChange::GroupCreated, tree, node, revision)
        }
        GroupCommand::Rename { node, name } => {
            let mut node = find(tree, &node)?.clone();
            let revision = client.rename_node(&node.id, &name).await?;
            node.name = name;
            changed(TreeChange::Renamed, tree, node, revision)
        }
        GroupCommand::Move {
            node,
            to,
            top,
            before,
            after,
        } => {
            let mut node = find(tree, &node)?.clone();
            let parent_id = match (to, top) {
                (Some(group), _) => Some(find(tree, &group)?.id.clone()),
                (None, true) => None,
                (None, false) => before
                    .as_deref()
                    .or(after.as_deref())
                    .map(|sibling| find(tree, sibling))
                    .transpose()?
                    .and_then(|sibling| sibling.parent_id.clone()),
            };
            let index = match (before, after) {
                (Some(sibling), _) => Some(index_at(tree, &node, parent_id.as_deref(), &sibling)?),
                (None, Some(sibling)) => {
                    Some(index_at(tree, &node, parent_id.as_deref(), &sibling)? + 1)
                }
                (None, None) => None,
            };
            let revision = client
                .move_node(&node.id, parent_id.as_deref(), index)
                .await?;
            node.parent_id = parent_id;
            changed(TreeChange::Moved, tree, node, revision)
        }
        GroupCommand::Rm { group } => {
            let node = find(tree, &group)?.clone();
            let revision = client.remove_group(&node.id).await?;
            changed(TreeChange::GroupRemoved, tree, node, revision)
        }
        GroupCommand::Ls { tree: false } => WorkspaceReport::Listed {
            workspace,
            listing: Listing::Groups,
        },
        GroupCommand::Ls { tree: true } => WorkspaceReport::Listed {
            workspace,
            listing: Listing::Tree,
        },
    };
    Ok(report.into())
}

/// The report of a change of `node`, which has its new name and parent.
/// `tree` is the tree before the change.
fn changed(change: TreeChange, tree: &WorkspaceTree, node: Node, revision: u64) -> WorkspaceReport {
    let parent_path = node
        .parent_id
        .as_deref()
        .map(|parent| format!("{}/", path_of(tree, parent)));
    let path = format!("{}{}", parent_path.unwrap_or_default(), node.name);
    WorkspaceReport::Changed {
        change,
        node,
        path,
        revision,
    }
}

/// The position of the sibling `reference` among the nodes of `parent_id`
/// without `node`: the index that puts `node` before it.
fn index_at(
    tree: &WorkspaceTree,
    node: &Node,
    parent_id: Option<&str>,
    reference: &str,
) -> Result<usize, ClientError> {
    let sibling = find(tree, reference)?;
    if sibling.id == node.id {
        return Err(ClientError::Failed(
            "a node cannot move before or after itself".to_owned(),
        ));
    }
    tree.children(parent_id)
        .iter()
        .filter(|id| **id != node.id)
        .position(|id| *id == sibling.id)
        .ok_or_else(|| {
            let target =
                parent_id.map_or_else(|| "the top level".to_owned(), |id| path_of(tree, id));
            ClientError::Failed(format!("{} is not in {target}", path_of(tree, &sibling.id)))
        })
}

/// The directory of a new Project. On the local host: the absolute path of
/// `requested` or of the current directory, or the root of its git
/// repository. On a remote host: `requested`, an absolute path there.
fn project_directory(
    requested: Option<PathBuf>,
    on_remote_host: bool,
) -> Result<PathBuf, ClientError> {
    if on_remote_host {
        return match requested {
            Some(directory) if directory.is_absolute() => Ok(directory),
            _ => Err(ClientError::Failed(
                "with --host, give the directory as an absolute path on the host".to_owned(),
            )),
        };
    }
    let directory = match requested {
        Some(directory) => path::absolute(directory),
        None => env::current_dir(),
    }
    .map_err(|error| ClientError::Failed(format!("the directory is not valid: {error}")))?;
    Ok(repository_root(&directory).unwrap_or(directory))
}

/// The root of the git repository of `directory`. `None` for a directory
/// outside a repository, and on a computer without git: the directory
/// itself is the Project then.
fn repository_root(directory: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["rev-parse", "--show-toplevel"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?;
    Some(PathBuf::from(root.trim_end_matches('\n')))
}

/// The default name of a Project: the last part of its directory.
fn last_part(directory: &Path) -> Result<String, ClientError> {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            ClientError::Failed(format!(
                "{} has no name for a Project: give one with --name",
                directory.display()
            ))
        })
}
