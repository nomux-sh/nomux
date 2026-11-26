//! The workspace tree of the host: the Groups and the Projects. The daemon
//! owns the tree, stores it with its revision, and sends an event for each
//! change. `spec/workspace.md` of the `proto` repository has the rules, and
//! `docs/workspace.md` describes this part.
//!
//! - `limits`: the limits of a name, of a directory and of the tree.
//! - `tree_file`: the tree on disk.
//!
//! The rules of the tree itself are in `nomux_core::workspace_tree`: the
//! clients apply the events with them.

mod limits;
mod tree_file;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_core::workspace_tree::{Node, NodeKind, TreeError, WorkspaceTree};
use nomux_proto::wire::v1::{
    Error, ErrorCode, WorkspaceAddProjectRequest, WorkspaceAddProjectResponse, WorkspaceChanged,
    WorkspaceCreateGroupRequest, WorkspaceCreateGroupResponse, WorkspaceGroup,
    WorkspaceMoveRequest, WorkspaceNode, WorkspaceNodeAdded, WorkspaceNodeMoved,
    WorkspaceNodeRemoved, WorkspaceNodeRenamed, WorkspaceProject, WorkspaceRenameRequest,
    WorkspaceTreeResponse, event, workspace_changed::Change, workspace_node,
};

use self::limits::MAX_NODES;
use self::tree_file::TreeFile;
use crate::events::EventLog;
use crate::session_id;

/// The number of characters of a node identifier that the daemon makes:
/// 2^60 identifiers, thus a removed node does not give its identifier to a
/// new one.
const NODE_IDENTIFIER_LENGTH: usize = 12;

pub(crate) struct Workspace {
    current: Mutex<Current>,
    file: TreeFile,
    events: Arc<EventLog>,
}

/// The tree and its revision. They change together.
struct Current {
    tree: WorkspaceTree,
    revision: u64,
}

impl Workspace {
    /// The tree that the state directory `state_directory` has.
    pub(crate) fn open(state_directory: &Path, events: Arc<EventLog>) -> Self {
        let file = TreeFile::new(state_directory);
        let loaded = file.load();
        Self {
            current: Mutex::new(Current {
                tree: loaded.tree,
                revision: loaded.revision,
            }),
            file,
            events,
        }
    }

    pub(crate) fn tree(&self) -> WorkspaceTreeResponse {
        let current = self.current();
        WorkspaceTreeResponse {
            revision: current.revision,
            nodes: current.tree.nodes().into_iter().map(wire_node).collect(),
        }
    }

    pub(crate) fn add_project(
        &self,
        request: WorkspaceAddProjectRequest,
    ) -> Result<WorkspaceAddProjectResponse, Error> {
        let directory = resolved_directory(&request.directory)?;
        let kind = NodeKind::Project { directory };
        let (revision, node) = self.add(request.name, &request.parent_id, request.index, kind)?;
        Ok(WorkspaceAddProjectResponse {
            revision,
            node: Some(node),
        })
    }

    pub(crate) fn create_group(
        &self,
        request: WorkspaceCreateGroupRequest,
    ) -> Result<WorkspaceCreateGroupResponse, Error> {
        let (revision, node) = self.add(
            request.name,
            &request.parent_id,
            request.index,
            NodeKind::Group,
        )?;
        Ok(WorkspaceCreateGroupResponse {
            revision,
            node: Some(node),
        })
    }

    /// Removes the node `id`, which must be a Group if `group` is `true`
    /// and a Project if not. Returns the revision.
    pub(crate) fn remove(&self, id: &str, group: bool) -> Result<u64, Error> {
        self.change(|tree| {
            // A refused change does not reach the tree: `tree` is a copy.
            let node = tree.remove(id).map_err(tree_error)?;
            match (node.is_group(), group) {
                (true, false) => return Err(failed(format!("{id} is a Group, not a Project"))),
                (false, true) => return Err(failed(format!("{id} is a Project, not a Group"))),
                _ => {}
            }
            Ok(Some(Change::Removed(WorkspaceNodeRemoved {
                id: id.to_owned(),
            })))
        })
    }

    pub(crate) fn rename(&self, request: &WorkspaceRenameRequest) -> Result<u64, Error> {
        limits::check_name(&request.name).map_err(failed)?;
        self.change(|tree| {
            let changed = tree
                .rename(&request.id, &request.name)
                .map_err(tree_error)?;
            Ok(changed.then(|| {
                Change::Renamed(WorkspaceNodeRenamed {
                    id: request.id.clone(),
                    name: request.name.clone(),
                })
            }))
        })
    }

    pub(crate) fn move_node(&self, request: &WorkspaceMoveRequest) -> Result<u64, Error> {
        self.change(|tree| {
            let index = tree
                .move_node(
                    &request.id,
                    parent(&request.parent_id),
                    requested_index(request.index),
                )
                .map_err(tree_error)?;
            Ok(index.map(|index| {
                Change::Moved(WorkspaceNodeMoved {
                    id: request.id.clone(),
                    parent_id: request.parent_id.clone(),
                    index: wire_index(index),
                })
            }))
        })
    }

    fn add(
        &self,
        name: String,
        parent_id: &str,
        index: Option<u32>,
        kind: NodeKind,
    ) -> Result<(u64, WorkspaceNode), Error> {
        limits::check_name(&name).map_err(failed)?;
        let mut added = None;
        let revision = self.change(|tree| {
            if tree.len() >= MAX_NODES {
                return Err(failed(format!(
                    "the workspace tree has {MAX_NODES} nodes, the limit: remove a node first"
                )));
            }
            let node = Node {
                id: new_identifier(tree)?,
                parent_id: parent(parent_id).map(str::to_owned),
                name,
                kind,
            };
            if let Some(directory) = node.directory()
                && let Some(project) = project_with_directory(tree, directory)
            {
                return Err(failed(format!(
                    "the Project {} has the directory {} already",
                    project.id,
                    directory.display()
                )));
            }
            let wire = wire_node(&node);
            let index = tree
                .insert(node, requested_index(index))
                .map_err(tree_error)?;
            added = Some(wire.clone());
            Ok(Some(Change::Added(WorkspaceNodeAdded {
                node: Some(wire),
                index: wire_index(index),
            })))
        })?;
        Ok((
            revision,
            added.expect("a node that is added makes a change"),
        ))
    }

    /// Changes the tree with `edit`, which gives the change or `None` if
    /// nothing changes. The tree changes only if the file stores it. Returns
    /// the revision after the change.
    fn change(
        &self,
        edit: impl FnOnce(&mut WorkspaceTree) -> Result<Option<Change>, Error>,
    ) -> Result<u64, Error> {
        let mut current = self.current();
        let mut tree = current.tree.clone();
        let Some(change) = edit(&mut tree)? else {
            return Ok(current.revision);
        };
        let revision = current.revision + 1;
        self.file.write(&tree, revision).map_err(|error| {
            failed(format!(
                "the workspace tree did not change: the daemon cannot store it: {error}"
            ))
        })?;
        *current = Current { tree, revision };
        drop(current);
        self.events
            .publish(event::Body::WorkspaceChanged(WorkspaceChanged {
                revision,
                change: Some(change),
            }));
        Ok(revision)
    }

    fn current(&self) -> MutexGuard<'_, Current> {
        // No code panics while it holds the lock, thus the tree is whole.
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The directory of a new Project: `requested` with each symbolic link
/// resolved.
fn resolved_directory(requested: &str) -> Result<PathBuf, Error> {
    if !Path::new(requested).is_absolute() {
        return Err(failed(format!("{requested:?} is not an absolute path")));
    }
    let directory = fs::canonicalize(requested)
        .map_err(|error| failed(format!("the directory {requested} cannot be used: {error}")))?;
    if !directory.is_dir() {
        return Err(failed(format!("{requested} is not a directory")));
    }
    let text = directory.to_str().ok_or_else(|| {
        failed(format!(
            "the path of {requested} is not UTF-8 after its symbolic links are resolved"
        ))
    })?;
    limits::check_directory(text).map_err(failed)?;
    Ok(directory)
}

fn project_with_directory<'tree>(
    tree: &'tree WorkspaceTree,
    directory: &Path,
) -> Option<&'tree Node> {
    tree.nodes()
        .into_iter()
        .find(|node| node.directory() == Some(directory))
}

fn new_identifier(tree: &WorkspaceTree) -> Result<String, Error> {
    loop {
        let id = session_id::random_text(NODE_IDENTIFIER_LENGTH).map_err(|error| {
            failed(format!("the daemon cannot make a node identifier: {error}"))
        })?;
        if tree.get(&id).is_none() {
            return Ok(id);
        }
    }
}

/// The parent of a request: an empty identifier is the top level.
fn parent(parent_id: &str) -> Option<&str> {
    (!parent_id.is_empty()).then_some(parent_id)
}

fn requested_index(index: Option<u32>) -> Option<usize> {
    index.map(|index| usize::try_from(index).unwrap_or(usize::MAX))
}

fn wire_index(index: usize) -> u32 {
    u32::try_from(index).expect("the tree has fewer nodes than u32")
}

fn wire_node(node: &Node) -> WorkspaceNode {
    let kind = match &node.kind {
        NodeKind::Group => workspace_node::Kind::Group(WorkspaceGroup {}),
        NodeKind::Project { directory } => workspace_node::Kind::Project(WorkspaceProject {
            directory: directory.to_string_lossy().into_owned(),
        }),
    };
    WorkspaceNode {
        id: node.id.clone(),
        parent_id: node.parent_id.clone().unwrap_or_default(),
        name: node.name.clone(),
        kind: Some(kind),
    }
}

fn tree_error(error: TreeError) -> Error {
    match error {
        TreeError::NotFound(message) => Error {
            code: ErrorCode::NotFound.into(),
            message,
        },
        TreeError::Refused(message) => failed(message),
    }
}

fn failed(message: String) -> Error {
    Error {
        code: ErrorCode::Failed.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn open(directory: &Path) -> (Workspace, Arc<EventLog>) {
        let events = Arc::new(EventLog::default());
        (Workspace::open(directory, Arc::clone(&events)), events)
    }

    fn create_group(workspace: &Workspace, name: &str, parent_id: &str) -> Result<String, Error> {
        let created = workspace.create_group(WorkspaceCreateGroupRequest {
            name: name.to_owned(),
            parent_id: parent_id.to_owned(),
            index: None,
        })?;
        Ok(created.node.unwrap().id)
    }

    fn add_project(workspace: &Workspace, directory: &Path) -> Result<String, Error> {
        let added = workspace.add_project(WorkspaceAddProjectRequest {
            directory: directory.to_str().unwrap().to_owned(),
            name: "web".to_owned(),
            parent_id: String::new(),
            index: None,
        })?;
        Ok(added.node.unwrap().id)
    }

    #[test]
    fn a_restart_keeps_the_tree_and_its_revision() {
        let state = tempfile::tempdir().unwrap();
        let project_directory = tempfile::tempdir().unwrap();
        let (workspace, events) = open(state.path());
        let group = create_group(&workspace, "Client work", "").unwrap();
        let project = add_project(&workspace, project_directory.path()).unwrap();
        workspace
            .move_node(&WorkspaceMoveRequest {
                id: project,
                parent_id: group,
                index: None,
            })
            .unwrap();
        let before = workspace.tree();
        assert_eq!(before.revision, 3);
        assert_eq!(events.last_seq(), 3);
        drop(workspace);

        let (workspace, _) = open(state.path());

        assert_eq!(workspace.tree(), before);
        assert_eq!(create_group(&workspace, "Archive", "").map(drop), Ok(()));
        assert_eq!(workspace.tree().revision, 4);
    }

    #[test]
    fn a_damaged_or_newer_file_goes_aside_and_the_daemon_starts_with_an_empty_tree() {
        for content in [
            "{\"version\":1,",
            "{\"version\":2,\"revision\":9,\"nodes\":[]}",
        ] {
            let state = tempfile::tempdir().unwrap();
            let (workspace, _) = open(state.path());
            create_group(&workspace, "Client work", "").unwrap();
            let file = state.path().join("workspace.json");
            fs::write(&file, content).unwrap();

            let (workspace, _) = open(state.path());

            let tree = workspace.tree();
            assert!(tree.nodes.is_empty(), "{content}");
            assert!(tree.revision > 1, "the revision decreased: {content}");
            let kept: Vec<_> = fs::read_dir(state.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.to_string_lossy().ends_with(".damaged"))
                .collect();
            assert_eq!(kept.len(), 1, "{content}");
            assert_eq!(fs::read_to_string(&kept[0]).unwrap(), content);
            assert!(!file.exists());
        }
    }

    #[test]
    fn the_limits_refuse_a_node_and_a_second_project_of_a_directory() {
        let state = tempfile::tempdir().unwrap();
        let project_directory = tempfile::tempdir().unwrap();
        let link = state.path().join("link");
        symlink(project_directory.path(), &link).unwrap();
        let (workspace, _) = open(state.path());
        let project = add_project(&workspace, project_directory.path()).unwrap();

        let second = add_project(&workspace, &link).unwrap_err();
        assert_eq!(second.code, i32::from(ErrorCode::Failed));
        assert!(second.message.contains(&project), "{}", second.message);
        for name in ["", &"n".repeat(129), "a\nb"] {
            assert!(create_group(&workspace, name, "").is_err(), "{name:?}");
        }
        for index in 1..MAX_NODES {
            create_group(&workspace, &index.to_string(), "").unwrap();
        }
        let full = create_group(&workspace, "one more", "").unwrap_err();
        assert!(full.message.contains("800 nodes"), "{}", full.message);
        assert_eq!(
            workspace.rename(&WorkspaceRenameRequest {
                id: project,
                name: "api".to_owned(),
            }),
            Ok(MAX_NODES as u64 + 1)
        );
    }
}
