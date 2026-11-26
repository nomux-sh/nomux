//! The requests of a client for the workspace tree of a host.

use std::path::Path;

use nomux_core::workspace_tree::Node;
use nomux_proto::wire::v1::{
    WorkspaceAddProjectRequest, WorkspaceCreateGroupRequest, WorkspaceMoveRequest, WorkspaceNode,
    WorkspaceRemoveGroupRequest, WorkspaceRemoveProjectRequest, WorkspaceRenameRequest,
    WorkspaceTreeRequest, request, response,
};

use crate::client::unexpected_result;
use crate::extension_requests::host_path;
use crate::workspace::node_of;
use crate::{Client, ClientError, Workspace};

impl Client {
    /// The workspace tree of the host and its revision.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn workspace(&mut self) -> Result<Workspace, ClientError> {
        match self
            .call(request::Body::WorkspaceTree(WorkspaceTreeRequest {}))
            .await?
        {
            response::Result::WorkspaceTree(tree) => tree.try_into(),
            _ => Err(unexpected_result()),
        }
    }

    /// Adds a Project with the directory `directory`, an absolute path on
    /// the host, to the Group `parent_id` or to the top level, at `index`
    /// or as the last node. Returns the revision and the new node.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier
    /// `parent_id`, and [`ClientError::Failed`] if the directory or the name
    /// is not valid, if a Project has the directory already, or if the tree
    /// is full.
    pub async fn add_project(
        &mut self,
        directory: &Path,
        name: &str,
        parent_id: Option<&str>,
        index: Option<usize>,
    ) -> Result<(u64, Node), ClientError> {
        let request = WorkspaceAddProjectRequest {
            directory: host_path(directory)?,
            name: name.to_owned(),
            parent_id: parent_id.unwrap_or_default().to_owned(),
            index: wire_index(index),
        };
        match self
            .call(request::Body::WorkspaceAddProject(request))
            .await?
        {
            response::Result::WorkspaceAddProject(added) => added_node(added.revision, added.node),
            _ => Err(unexpected_result()),
        }
    }

    /// Adds an empty Group to the Group `parent_id` or to the top level, at
    /// `index` or as the last node. Returns the revision and the new node.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier
    /// `parent_id`, and [`ClientError::Failed`] if the name is not valid or
    /// if the tree is full.
    pub async fn create_group(
        &mut self,
        name: &str,
        parent_id: Option<&str>,
        index: Option<usize>,
    ) -> Result<(u64, Node), ClientError> {
        let request = WorkspaceCreateGroupRequest {
            name: name.to_owned(),
            parent_id: parent_id.unwrap_or_default().to_owned(),
            index: wire_index(index),
        };
        match self
            .call(request::Body::WorkspaceCreateGroup(request))
            .await?
        {
            response::Result::WorkspaceCreateGroup(created) => {
                added_node(created.revision, created.node)
            }
            _ => Err(unexpected_result()),
        }
    }

    /// Removes the Project `id` from the tree. No file and no session
    /// changes. Returns the revision.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier, and
    /// [`ClientError::Failed`] if the node is a Group.
    pub async fn remove_project(&mut self, id: &str) -> Result<u64, ClientError> {
        let request = WorkspaceRemoveProjectRequest { id: id.to_owned() };
        match self
            .call(request::Body::WorkspaceRemoveProject(request))
            .await?
        {
            response::Result::WorkspaceRemoveProject(removed) => Ok(removed.revision),
            _ => Err(unexpected_result()),
        }
    }

    /// Removes the Group `id`. Its content takes its place in its parent.
    /// Returns the revision.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier, and
    /// [`ClientError::Failed`] if the node is a Project.
    pub async fn remove_group(&mut self, id: &str) -> Result<u64, ClientError> {
        let request = WorkspaceRemoveGroupRequest { id: id.to_owned() };
        match self
            .call(request::Body::WorkspaceRemoveGroup(request))
            .await?
        {
            response::Result::WorkspaceRemoveGroup(removed) => Ok(removed.revision),
            _ => Err(unexpected_result()),
        }
    }

    /// Gives the node `id` the name `name`. Returns the revision.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no node has the identifier, and
    /// [`ClientError::Failed`] if the name is not valid.
    pub async fn rename_node(&mut self, id: &str, name: &str) -> Result<u64, ClientError> {
        let request = WorkspaceRenameRequest {
            id: id.to_owned(),
            name: name.to_owned(),
        };
        match self.call(request::Body::WorkspaceRename(request)).await? {
            response::Result::WorkspaceRename(renamed) => Ok(renamed.revision),
            _ => Err(unexpected_result()),
        }
    }

    /// Puts the node `id` in the Group `parent_id` or at the top level, at
    /// `index` among its new siblings or as the last node. Returns the
    /// revision.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if a node does not exist, and
    /// [`ClientError::Failed`] if `parent_id` is a Project, the node itself
    /// or a Group below it.
    pub async fn move_node(
        &mut self,
        id: &str,
        parent_id: Option<&str>,
        index: Option<usize>,
    ) -> Result<u64, ClientError> {
        let request = WorkspaceMoveRequest {
            id: id.to_owned(),
            parent_id: parent_id.unwrap_or_default().to_owned(),
            index: wire_index(index),
        };
        match self.call(request::Body::WorkspaceMove(request)).await? {
            response::Result::WorkspaceMove(moved) => Ok(moved.revision),
            _ => Err(unexpected_result()),
        }
    }
}

fn added_node(revision: u64, node: Option<WorkspaceNode>) -> Result<(u64, Node), ClientError> {
    let node = node.ok_or_else(|| ClientError::invalid_reply("the response has no node"))?;
    Ok((revision, node_of(node)?))
}

/// An index larger than `u32` is larger than the number of siblings: the
/// node is the last.
fn wire_index(index: Option<usize>) -> Option<u32> {
    index.map(|index| u32::try_from(index).unwrap_or(u32::MAX))
}
