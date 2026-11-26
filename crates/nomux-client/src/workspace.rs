//! The workspace tree of a host: the data of the Workspace service, and the
//! copy of a client that the events keep current.

use nomux_core::workspace_tree::{Node, NodeKind, WorkspaceTree};
use nomux_proto::wire::v1::{self as wire, workspace_changed, workspace_node};

use crate::{ClientError, Session};

/// The workspace tree of a host and its revision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Workspace {
    /// The revision of the tree. It increases by 1 with each change and
    /// never decreases.
    pub revision: u64,
    /// The Groups and the Projects.
    pub tree: WorkspaceTree,
}

/// One change of the tree: the content of an
/// [`EventBody::WorkspaceChanged`](crate::EventBody::WorkspaceChanged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceChange {
    /// A node came into the tree at `index` among its siblings.
    Added {
        /// The new node.
        node: Node,
        /// The position of the node.
        index: usize,
    },
    /// A node has a new name.
    Renamed {
        /// The identifier of the node.
        id: String,
        /// The new name.
        name: String,
    },
    /// A node has a new parent or a new position.
    Moved {
        /// The identifier of the node.
        id: String,
        /// The new parent. `None` for the top level.
        parent_id: Option<String>,
        /// The new position among the siblings.
        index: usize,
    },
    /// A node went out of the tree. The content of a Group takes its place.
    Removed {
        /// The identifier of the node.
        id: String,
    },
    /// A change that this client does not know.
    Unknown,
}

/// What [`Workspace::apply`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The tree has the change now.
    Changed,
    /// The tree had the change already.
    Included,
    /// The client missed a change, or the change does not fit the tree:
    /// ask for the tree again with [`Client::workspace`](crate::Client::workspace).
    Missed,
}

impl Workspace {
    /// Applies the change of the event with `revision` to this copy of the
    /// tree.
    pub fn apply(&mut self, revision: u64, change: &WorkspaceChange) -> Applied {
        if revision <= self.revision {
            return Applied::Included;
        }
        if revision > self.revision + 1 {
            return Applied::Missed;
        }
        let tree = &mut self.tree;
        let result = match change {
            WorkspaceChange::Added { node, index } => {
                tree.insert(node.clone(), Some(*index)).map(drop)
            }
            WorkspaceChange::Renamed { id, name } => tree.rename(id, name).map(drop),
            WorkspaceChange::Moved {
                id,
                parent_id,
                index,
            } => tree
                .move_node(id, parent_id.as_deref(), Some(*index))
                .map(drop),
            WorkspaceChange::Removed { id } => tree.remove(id).map(drop),
            WorkspaceChange::Unknown => return Applied::Missed,
        };
        match result {
            Ok(()) => {
                self.revision = revision;
                Applied::Changed
            }
            Err(_) => Applied::Missed,
        }
    }
}

impl Session {
    /// The Project of this session in `tree`: the Project whose directory is
    /// the resolved working directory of the session or a directory above
    /// it. Without a resolved path, the working directory as it was given.
    /// `None` if the session has no Project.
    #[must_use]
    pub fn project<'tree>(&self, tree: &'tree WorkspaceTree) -> Option<&'tree Node> {
        let directory = self
            .resolved_working_directory
            .as_deref()
            .or(self.working_directory.as_deref())?;
        tree.project_of(directory)
    }
}

impl TryFrom<wire::WorkspaceTreeResponse> for Workspace {
    type Error = ClientError;

    fn try_from(response: wire::WorkspaceTreeResponse) -> Result<Self, ClientError> {
        let nodes = response
            .nodes
            .into_iter()
            .map(node_of)
            .collect::<Result<Vec<_>, _>>()?;
        let tree = WorkspaceTree::from_nodes(nodes)
            .map_err(|_| ClientError::invalid_reply("the workspace tree breaks its rules"))?;
        Ok(Self {
            revision: response.revision,
            tree,
        })
    }
}

/// The node of a response or of an event.
pub(crate) fn node_of(node: wire::WorkspaceNode) -> Result<Node, ClientError> {
    let kind = match node.kind {
        Some(workspace_node::Kind::Group(_)) => NodeKind::Group,
        Some(workspace_node::Kind::Project(project)) => NodeKind::Project {
            directory: project.directory.into(),
        },
        None => return Err(ClientError::invalid_reply("a workspace node has no kind")),
    };
    Ok(Node {
        id: node.id,
        parent_id: parent_of(node.parent_id),
        name: node.name,
        kind,
    })
}

/// The change of an event.
pub(crate) fn change_of(change: Option<workspace_changed::Change>) -> WorkspaceChange {
    let index = |index: u32| usize::try_from(index).unwrap_or(usize::MAX);
    match change {
        Some(workspace_changed::Change::Added(added)) => {
            match added.node.map(node_of).transpose() {
                Ok(Some(node)) => WorkspaceChange::Added {
                    node,
                    index: index(added.index),
                },
                _ => WorkspaceChange::Unknown,
            }
        }
        Some(workspace_changed::Change::Renamed(renamed)) => WorkspaceChange::Renamed {
            id: renamed.id,
            name: renamed.name,
        },
        Some(workspace_changed::Change::Moved(moved)) => WorkspaceChange::Moved {
            id: moved.id,
            parent_id: parent_of(moved.parent_id),
            index: index(moved.index),
        },
        Some(workspace_changed::Change::Removed(removed)) => {
            WorkspaceChange::Removed { id: removed.id }
        }
        None => WorkspaceChange::Unknown,
    }
}

/// An empty parent identifier is the top level.
fn parent_of(parent_id: String) -> Option<String> {
    (!parent_id.is_empty()).then_some(parent_id)
}
