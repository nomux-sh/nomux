//! The JSON form of the report of a `project` or `group` command.
//! `docs/cli.md` has the schema.

use std::borrow::Cow;

use nomux_client::{Node, Session, WorkspaceTree};
use serde::Serialize;

use super::workspace_human::listed_nodes;
use crate::workspace_commands::WorkspaceReport;
use crate::workspace_commands::node_path::path_of;

/// Each variant is one top-level field of the document.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WorkspaceBody<'a> {
    Change(ChangeDocument<'a>),
    Workspace(WorkspaceDocument<'a>),
}

impl<'a> WorkspaceBody<'a> {
    pub(super) fn of(report: &'a WorkspaceReport) -> Self {
        match report {
            WorkspaceReport::Changed {
                change,
                node,
                path,
                revision,
            } => Self::Change(ChangeDocument {
                kind: change.name(),
                revision: *revision,
                node: NodeDocument::new(node, Some(Cow::Borrowed(path))),
            }),
            WorkspaceReport::Listed { workspace, listing } => Self::Workspace(WorkspaceDocument {
                revision: workspace.revision,
                nodes: listed_nodes(workspace, *listing)
                    .into_iter()
                    .map(|node| NodeDocument::of(node, &workspace.tree))
                    .collect(),
            }),
        }
    }
}

#[derive(Serialize)]
pub(super) struct ChangeDocument<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    revision: u64,
    node: NodeDocument<'a>,
}

#[derive(Serialize)]
pub(super) struct WorkspaceDocument<'a> {
    revision: u64,
    nodes: Vec<NodeDocument<'a>>,
}

#[derive(Serialize)]
pub(super) struct NodeDocument<'a> {
    id: &'a str,
    parent_id: Option<&'a str>,
    name: &'a str,
    kind: &'static str,
    /// An event has no path: the client has no tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    directory: Option<Cow<'a, str>>,
}

impl<'a> NodeDocument<'a> {
    /// The document of `node` of `tree`.
    pub(super) fn of(node: &'a Node, tree: &WorkspaceTree) -> Self {
        Self::new(node, Some(Cow::Owned(path_of(tree, &node.id))))
    }

    /// The document of `node` of an event.
    pub(super) fn without_path(node: &'a Node) -> Self {
        Self::new(node, None)
    }

    fn new(node: &'a Node, path: Option<Cow<'a, str>>) -> Self {
        Self {
            id: &node.id,
            parent_id: node.parent_id.as_deref(),
            name: &node.name,
            kind: if node.is_group() { "group" } else { "project" },
            path,
            directory: node
                .directory()
                .map(|directory| directory.to_string_lossy()),
        }
    }
}

/// The Project of a session.
#[derive(Serialize)]
pub(super) struct ProjectDocument<'a> {
    id: &'a str,
    path: String,
}

impl<'a> ProjectDocument<'a> {
    /// The Project of `session` in `tree`. `None` if the session has none.
    pub(super) fn of_session(session: &Session, tree: &'a WorkspaceTree) -> Option<Self> {
        let project = tree.project_of(session.working_directory.as_deref()?)?;
        Some(Self {
            id: &project.id,
            path: path_of(tree, &project.id),
        })
    }
}
