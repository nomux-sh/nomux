//! The result of a `project` or `group` command that did not fail.

use nomux_client::{Node, Workspace};

use crate::report::Report;

#[derive(Debug)]
pub(crate) enum WorkspaceReport {
    /// The command changed the tree.
    Changed {
        change: TreeChange,
        /// The node with its name and its parent after the change.
        node: Node,
        /// The path of the node after the change. A removed node has the
        /// path that it had.
        path: String,
        revision: u64,
    },
    /// The tree, for a listing.
    Listed {
        workspace: Workspace,
        listing: Listing,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeChange {
    ProjectAdded,
    ProjectRemoved,
    GroupCreated,
    GroupRemoved,
    Renamed,
    Moved,
}

/// What a listing shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listing {
    /// `project ls`: the Projects.
    Projects,
    /// `group ls`: the Groups.
    Groups,
    /// `group ls --tree`: each node, below its Group.
    Tree,
}

impl TreeChange {
    /// The name of the change in the text and in the JSON document.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::ProjectAdded => "project_added",
            Self::ProjectRemoved => "project_removed",
            Self::GroupCreated => "group_created",
            Self::GroupRemoved => "group_removed",
            Self::Renamed => "renamed",
            Self::Moved => "moved",
        }
    }
}

impl From<WorkspaceReport> for Report {
    fn from(report: WorkspaceReport) -> Self {
        Self::Workspace(report)
    }
}
