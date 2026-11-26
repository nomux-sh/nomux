//! The path of a node: the names from the top level down to it, with `/`
//! between them. A person names a node with its path or its identifier.

use nomux_client::{ClientError, Node, WorkspaceTree};

const SEPARATOR: &str = "/";

/// The path of the node `id`, for example `Client work/Acme/acme-web`.
pub(crate) fn path_of(tree: &WorkspaceTree, id: &str) -> String {
    let mut names = Vec::new();
    let mut current = tree.get(id);
    while let Some(node) = current {
        names.push(node.name.as_str());
        current = node
            .parent_id
            .as_deref()
            .and_then(|parent| tree.get(parent));
    }
    names.reverse();
    names.join(SEPARATOR)
}

/// The number of Groups above `node`.
pub(crate) fn depth_of(tree: &WorkspaceTree, node: &Node) -> usize {
    let mut depth = 0;
    let mut parent = node.parent_id.as_deref();
    while let Some(id) = parent {
        depth += 1;
        parent = tree.get(id).and_then(|group| group.parent_id.as_deref());
    }
    depth
}

/// The node that `reference` names: the node with the identifier, else the
/// node with the path, else the node with the name.
///
/// # Errors
///
/// Returns [`ClientError::NotFound`] if no node has the reference, and
/// [`ClientError::Failed`] with the candidates if more than one node has it.
pub(crate) fn find<'tree>(
    tree: &'tree WorkspaceTree,
    reference: &str,
) -> Result<&'tree Node, ClientError> {
    if let Some(node) = tree.get(reference) {
        return Ok(node);
    }
    let nodes = tree.nodes();
    let mut candidates: Vec<&Node> = nodes
        .iter()
        .copied()
        .filter(|node| path_of(tree, &node.id) == reference)
        .collect();
    if candidates.is_empty() {
        candidates = nodes
            .into_iter()
            .filter(|node| node.name == reference)
            .collect();
    }
    match candidates.as_slice() {
        [] => Err(ClientError::NotFound(format!(
            "no node of the workspace tree has the identifier, the path or the name {reference:?}"
        ))),
        [node] => Ok(node),
        nodes => {
            let lines: Vec<String> = nodes
                .iter()
                .map(|node| format!("  {}  {}", node.id, path_of(tree, &node.id)))
                .collect();
            Err(ClientError::Failed(format!(
                "{reference:?} names {} nodes: give the identifier of one\n{}",
                nodes.len(),
                lines.join("\n")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use nomux_client::NodeKind;

    use super::*;

    fn node(id: &str, parent_id: Option<&str>, name: &str) -> Node {
        Node {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: name.to_owned(),
            kind: NodeKind::Group,
        }
    }

    #[test]
    fn a_node_is_found_by_its_identifier_its_path_or_its_name_and_an_ambiguous_name_is_refused() {
        let tree = WorkspaceTree::from_nodes([
            node("g1", None, "Client work"),
            node("g2", Some("g1"), "Acme"),
            node("p1", Some("g2"), "web"),
            node("g3", None, "Archive"),
            node("p2", Some("g3"), "web"),
        ])
        .unwrap();

        assert_eq!(find(&tree, "p1").unwrap().id, "p1");
        assert_eq!(find(&tree, "Client work/Acme/web").unwrap().id, "p1");
        assert_eq!(find(&tree, "Acme").unwrap().id, "g2");
        assert!(matches!(
            find(&tree, "Acme/web"),
            Err(ClientError::NotFound(_))
        ));
        let Err(ClientError::Failed(message)) = find(&tree, "web") else {
            panic!("an ambiguous name is refused");
        };
        assert_eq!(
            message,
            "\"web\" names 2 nodes: give the identifier of one\n  p1  Client work/Acme/web\n  p2  Archive/web"
        );
    }
}
