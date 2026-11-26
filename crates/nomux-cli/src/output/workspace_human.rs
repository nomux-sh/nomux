//! The text form of the report of a `project` or `group` command, and of a
//! change of the workspace tree in `events --follow`.

use nomux_client::{Node, Workspace, WorkspaceChange};

use super::table::{Cell, INDENT, Table};
use crate::workspace_commands::node_path::{depth_of, path_of};
use crate::workspace_commands::{Listing, TreeChange, WorkspaceReport};

const NONE: &str = "-";

pub(super) fn report(report: &WorkspaceReport, width: Option<usize>) -> String {
    match report {
        WorkspaceReport::Changed {
            change,
            node,
            path,
            revision,
        } => format!(
            "{} {}: {path} (revision {revision})\n",
            changed(*change),
            node.id
        ),
        WorkspaceReport::Listed { workspace, listing } => listed(workspace, *listing, width),
    }
}

/// Only what a script needs: the identifiers.
pub(super) fn quiet_report(report: &WorkspaceReport) -> String {
    match report {
        WorkspaceReport::Changed { node, .. } => format!("{}\n", node.id),
        WorkspaceReport::Listed { workspace, listing } => listed_nodes(workspace, *listing)
            .into_iter()
            .flat_map(|node| [node.id.as_str(), "\n"])
            .collect(),
    }
}

/// One line for a change of the tree with `revision`.
pub(super) fn change_line(revision: u64, change: &WorkspaceChange) -> String {
    let what = match change {
        WorkspaceChange::Added { node, index } => {
            format!("added {} {:?} at position {index}", node.id, node.name)
        }
        WorkspaceChange::Renamed { id, name } => format!("renamed {id} to {name:?}"),
        WorkspaceChange::Moved {
            id,
            parent_id,
            index,
        } => format!(
            "moved {id} into {} at position {index}",
            parent_id.as_deref().unwrap_or("the top level")
        ),
        WorkspaceChange::Removed { id } => format!("removed {id}"),
        WorkspaceChange::Unknown => "a change that this client does not know".to_owned(),
    };
    format!("workspace revision {revision}: {what}")
}

fn changed(change: TreeChange) -> &'static str {
    match change {
        TreeChange::ProjectAdded => "added project",
        TreeChange::ProjectRemoved => "removed project",
        TreeChange::GroupCreated => "made group",
        TreeChange::GroupRemoved => "removed group",
        TreeChange::Renamed => "renamed",
        TreeChange::Moved => "moved",
    }
}

fn listed(workspace: &Workspace, listing: Listing, width: Option<usize>) -> String {
    let tree = &workspace.tree;
    let nodes = listed_nodes(workspace, listing);
    if nodes.is_empty() {
        return match listing {
            Listing::Projects => "no projects\n",
            Listing::Groups => "no groups\n",
            Listing::Tree => "the workspace tree is empty\n",
        }
        .to_owned();
    }
    let (header, shortened): (&[&str], &[usize]) = match listing {
        Listing::Projects => (&["ID", "PROJECT", "DIRECTORY"], &[2, 1]),
        Listing::Groups => (&["ID", "GROUP"], &[1]),
        Listing::Tree => (&["NAME", "KIND", "ID", "DIRECTORY"], &[3, 0]),
    };
    let mut table = Table::new(header).shorten(shortened);
    for node in nodes {
        let directory = node.directory().map_or_else(
            || NONE.to_owned(),
            |directory| directory.display().to_string(),
        );
        table.push(match listing {
            Listing::Projects => vec![
                Cell::new(&node.id),
                Cell::new(path_of(tree, &node.id)),
                Cell::new(directory),
            ],
            Listing::Groups => vec![Cell::new(&node.id), Cell::new(path_of(tree, &node.id))],
            Listing::Tree => vec![
                Cell::new(format!(
                    "{}{}",
                    INDENT.repeat(depth_of(tree, node)),
                    node.name
                )),
                Cell::new(if node.is_group() { "group" } else { "project" }),
                Cell::new(&node.id),
                Cell::new(directory),
            ],
        });
    }
    table.render(width)
}

/// The nodes of a listing, in the sequence of the tree.
pub(super) fn listed_nodes(workspace: &Workspace, listing: Listing) -> Vec<&Node> {
    let nodes = workspace.tree.nodes().into_iter();
    match listing {
        Listing::Projects => nodes.filter(|node| !node.is_group()).collect(),
        Listing::Groups => nodes.filter(|node| node.is_group()).collect(),
        Listing::Tree => nodes.collect(),
    }
}

#[cfg(test)]
mod tests {
    use nomux_client::{NodeKind, WorkspaceTree};

    use super::super::table::assert_fits;
    use super::*;

    fn node(id: &str, parent_id: Option<&str>, name: &str, directory: Option<&str>) -> Node {
        Node {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: name.to_owned(),
            kind: directory.map_or(NodeKind::Group, |directory| NodeKind::Project {
                directory: directory.into(),
            }),
        }
    }

    #[test]
    fn each_listing_fits_a_terminal_of_60_columns() {
        let tree = WorkspaceTree::from_nodes([
            node("g1", None, "customer-projects-of-the-year", None),
            node("g2", Some("g1"), "infrastructure-and-deployment", None),
            node(
                "p1",
                Some("g2"),
                "kubernetes-cluster-configuration",
                Some("/home/user/work/customers/infrastructure/kubernetes-cluster"),
            ),
        ])
        .expect("a valid tree");
        let workspace = Workspace { revision: 3, tree };

        for listing in [Listing::Projects, Listing::Groups, Listing::Tree] {
            let text = listed(&workspace, listing, Some(60));
            assert_fits(&text, 60);
            assert!(text.contains('\u{2026}'), "{text}");
        }
    }
}
