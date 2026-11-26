//! The workspace tree: the Groups and the Projects of a host, and the rules
//! that change it. The daemon owns the tree. A client keeps a copy and
//! applies each change of the daemon with the same rules.
//! `spec/workspace.md` of the `proto` repository has the rules.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// One node of the tree: a Group or a Project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The identifier. It does not change.
    pub id: String,
    /// The identifier of the Group that holds the node. `None` at the top
    /// level.
    pub parent_id: Option<String>,
    /// The name. Two nodes can have the same name.
    pub name: String,
    /// What the node is.
    pub kind: NodeKind,
}

/// What a node is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// A Group: it holds Projects and Groups.
    Group,
    /// A Project: a directory on the host.
    Project {
        /// The absolute path of the directory, with each symbolic link
        /// resolved.
        directory: PathBuf,
    },
}

impl Node {
    /// `true` if the node is a Group.
    #[must_use]
    pub fn is_group(&self) -> bool {
        self.kind == NodeKind::Group
    }

    /// The directory of a Project. `None` for a Group.
    #[must_use]
    pub fn directory(&self) -> Option<&Path> {
        match &self.kind {
            NodeKind::Group => None,
            NodeKind::Project { directory } => Some(directory),
        }
    }
}

/// Why the tree did not change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// No node has the identifier.
    NotFound(String),
    /// The rules of the tree do not permit the change.
    Refused(String),
}

impl fmt::Display for TreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(message) | Self::Refused(message) => formatter.write_str(message),
        }
    }
}

impl Error for TreeError {}

/// The Groups and the Projects of a host, each one with its position among
/// its siblings.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceTree {
    nodes: HashMap<String, Node>,
    /// The identifiers of the nodes of each parent, in their sequence. The
    /// key `None` is the top level.
    children: HashMap<Option<String>, Vec<String>>,
}

impl WorkspaceTree {
    /// The tree of `nodes`, which are in the sequence of a depth-first walk:
    /// a node comes after its parent, and the siblings are in their
    /// sequence.
    ///
    /// # Errors
    ///
    /// Returns an error if a node comes before its parent, if its parent is
    /// a Project, or if two nodes have the same identifier.
    pub fn from_nodes(nodes: impl IntoIterator<Item = Node>) -> Result<Self, TreeError> {
        let mut tree = Self::default();
        for node in nodes {
            tree.insert(node, None)?;
        }
        Ok(tree)
    }

    /// The number of nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` if the tree has no node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The node `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// The identifiers of the nodes in the Group `parent_id`, or at the top
    /// level for `None`, in their sequence.
    #[must_use]
    pub fn children(&self, parent_id: Option<&str>) -> &[String] {
        self.children
            .get(&parent_id.map(str::to_owned))
            .map_or(&[], Vec::as_slice)
    }

    /// The position of the node `id` among its siblings.
    #[must_use]
    pub fn position(&self, id: &str) -> Option<usize> {
        let node = self.nodes.get(id)?;
        self.children(node.parent_id.as_deref())
            .iter()
            .position(|sibling| sibling == id)
    }

    /// Each node in the sequence of a depth-first walk.
    #[must_use]
    pub fn nodes(&self) -> Vec<&Node> {
        let mut walk = Vec::with_capacity(self.nodes.len());
        // The stack has the siblings in the reverse sequence: the first one
        // is on top.
        let mut stack: Vec<&String> = self.children(None).iter().rev().collect();
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            walk.push(node);
            stack.extend(self.children(Some(id)).iter().rev());
        }
        walk
    }

    /// The Project of a session with `working_directory`: the Project with
    /// the longest directory that is equal to it or above it.
    #[must_use]
    pub fn project_of(&self, working_directory: &Path) -> Option<&Node> {
        self.nodes
            .values()
            .filter_map(|node| Some((node, node.directory()?)))
            .filter(|(_, directory)| working_directory.starts_with(directory))
            .max_by_key(|(_, directory)| directory.as_os_str().len())
            .map(|(node, _)| node)
    }

    /// Adds `node` to its parent at `index`, or as the last node if `index`
    /// is `None` or not smaller than the number of siblings. Returns the
    /// position of the node.
    ///
    /// # Errors
    ///
    /// Returns an error if a node has the identifier, or if the parent does
    /// not exist or is a Project.
    pub fn insert(&mut self, node: Node, index: Option<usize>) -> Result<usize, TreeError> {
        if self.nodes.contains_key(&node.id) {
            return Err(TreeError::Refused(format!(
                "a node has the identifier {} already",
                node.id
            )));
        }
        self.check_parent(node.parent_id.as_deref())?;
        let siblings = self.children.entry(node.parent_id.clone()).or_default();
        let index = index.map_or(siblings.len(), |index| index.min(siblings.len()));
        siblings.insert(index, node.id.clone());
        self.nodes.insert(node.id.clone(), node);
        Ok(index)
    }

    /// Gives the node `id` the name `name`. Returns `false` if the node has
    /// that name already.
    ///
    /// # Errors
    ///
    /// Returns an error if no node has the identifier.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<bool, TreeError> {
        let node = self.nodes.get_mut(id).ok_or_else(|| not_found(id))?;
        if node.name == name {
            return Ok(false);
        }
        name.clone_into(&mut node.name);
        Ok(true)
    }

    /// Puts the node `id` in the Group `parent_id`, or at the top level for
    /// `None`, at `index` among its new siblings without the node itself.
    /// Returns the new position, or `None` if the node has that place
    /// already.
    ///
    /// # Errors
    ///
    /// Returns an error if a node does not exist, if the parent is a
    /// Project, or if the parent is the node or a Group below it.
    pub fn move_node(
        &mut self,
        id: &str,
        parent_id: Option<&str>,
        index: Option<usize>,
    ) -> Result<Option<usize>, TreeError> {
        let old_parent = self
            .nodes
            .get(id)
            .ok_or_else(|| not_found(id))?
            .parent_id
            .clone();
        self.check_parent(parent_id)?;
        if self.is_in_subtree(parent_id, id) {
            return Err(TreeError::Refused(
                "a Group cannot move into itself or into a Group below it".to_owned(),
            ));
        }
        let old_index = self.position(id).ok_or_else(|| not_found(id))?;
        let new_parent = parent_id.map(str::to_owned);
        let siblings = self.children.entry(old_parent.clone()).or_default();
        siblings.remove(old_index);
        let siblings = self.children.entry(new_parent.clone()).or_default();
        let new_index = index.map_or(siblings.len(), |index| index.min(siblings.len()));
        siblings.insert(new_index, id.to_owned());
        if old_parent == new_parent && old_index == new_index {
            return Ok(None);
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.parent_id = new_parent;
        }
        Ok(Some(new_index))
    }

    /// Removes the node `id`. The nodes of a Group take the position of the
    /// Group in its parent, in their sequence. Returns the removed node.
    ///
    /// # Errors
    ///
    /// Returns an error if no node has the identifier.
    pub fn remove(&mut self, id: &str) -> Result<Node, TreeError> {
        let index = self.position(id).ok_or_else(|| not_found(id))?;
        let node = self.nodes.remove(id).ok_or_else(|| not_found(id))?;
        let content = self
            .children
            .remove(&Some(id.to_owned()))
            .unwrap_or_default();
        for child in &content {
            if let Some(child) = self.nodes.get_mut(child) {
                child.parent_id.clone_from(&node.parent_id);
            }
        }
        if let Some(siblings) = self.children.get_mut(&node.parent_id) {
            siblings.splice(index..=index, content);
        }
        Ok(node)
    }

    fn check_parent(&self, parent_id: Option<&str>) -> Result<(), TreeError> {
        let Some(parent_id) = parent_id else {
            return Ok(());
        };
        let parent = self
            .nodes
            .get(parent_id)
            .ok_or_else(|| not_found(parent_id))?;
        if parent.is_group() {
            Ok(())
        } else {
            Err(TreeError::Refused(format!(
                "{parent_id} is a Project: only a Group holds nodes"
            )))
        }
    }

    /// `true` if the node `start`, or a Group above it, is `id`.
    fn is_in_subtree(&self, start: Option<&str>, id: &str) -> bool {
        let mut current = start;
        while let Some(node_id) = current {
            if node_id == id {
                return true;
            }
            current = self.nodes[node_id].parent_id.as_deref();
        }
        false
    }
}

/// Two trees are equal if they have the same nodes at the same places.
impl PartialEq for WorkspaceTree {
    fn eq(&self, other: &Self) -> bool {
        self.nodes() == other.nodes()
    }
}

impl Eq for WorkspaceTree {}

fn not_found(id: &str) -> TreeError {
    TreeError::NotFound(format!(
        "no node of the workspace tree has the identifier {id}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, parent_id: Option<&str>) -> Node {
        Node {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: id.to_uppercase(),
            kind: NodeKind::Group,
        }
    }

    fn project(id: &str, parent_id: Option<&str>, directory: &str) -> Node {
        Node {
            kind: NodeKind::Project {
                directory: directory.into(),
            },
            ..group(id, parent_id)
        }
    }

    /// The tree of the examples of `spec/workspace.md`.
    fn example() -> WorkspaceTree {
        WorkspaceTree::from_nodes([
            group("g1", None),
            project("p1", Some("g1"), "/src/a/api"),
            project("p2", Some("g1"), "/src/a/web"),
            group("g2", None),
            group("g3", Some("g2")),
            project("p3", Some("g3"), "/src/b/old"),
            project("p4", None, "/home/user/notes"),
        ])
        .unwrap()
    }

    fn walk(tree: &WorkspaceTree) -> Vec<(&str, Option<&str>)> {
        tree.nodes()
            .into_iter()
            .map(|node| (node.id.as_str(), node.parent_id.as_deref()))
            .collect()
    }

    #[test]
    fn a_group_does_not_move_into_itself_or_below_itself_and_the_tree_stays() {
        let mut tree = example();
        tree.move_node("g1", Some("g2"), Some(0)).unwrap();

        for target in ["g1", "g2", "g3"] {
            let error = tree.move_node("g2", Some(target), None).unwrap_err();
            assert_eq!(
                error,
                TreeError::Refused(
                    "a Group cannot move into itself or into a Group below it".to_owned()
                ),
                "{target}"
            );
        }
        assert!(matches!(
            tree.move_node("g1", Some("p1"), None),
            Err(TreeError::Refused(_))
        ));
        assert_eq!(
            walk(&tree),
            [
                ("g2", None),
                ("g1", Some("g2")),
                ("p1", Some("g1")),
                ("p2", Some("g1")),
                ("g3", Some("g2")),
                ("p3", Some("g3")),
                ("p4", None),
            ]
        );
    }

    #[test]
    fn the_content_of_a_removed_group_takes_its_place_in_its_sequence() {
        let mut tree = example();
        tree.move_node("g1", Some("g2"), Some(0)).unwrap();

        let removed = tree.remove("g2").unwrap();

        assert_eq!(removed.id, "g2");
        assert_eq!(
            walk(&tree),
            [
                ("g1", None),
                ("p1", Some("g1")),
                ("p2", Some("g1")),
                ("g3", None),
                ("p3", Some("g3")),
                ("p4", None),
            ]
        );
        assert_eq!(tree.position("p4"), Some(2));
    }

    #[test]
    fn a_position_counts_the_siblings_without_the_node_and_the_same_place_changes_nothing() {
        let mut tree = example();

        // The siblings of `g1` at the top level without it are `g2` and `p4`.
        assert_eq!(tree.move_node("g1", None, Some(1)).unwrap(), Some(1));
        assert_eq!(tree.children(None), ["g2", "g1", "p4"]);
        assert_eq!(tree.move_node("g1", None, Some(1)).unwrap(), None);
        // A position after the last sibling puts the node last.
        assert_eq!(tree.move_node("g1", None, Some(99)).unwrap(), Some(2));
        assert_eq!(tree.insert(group("g4", Some("g1")), Some(0)).unwrap(), 0);
        assert_eq!(tree.children(Some("g1")), ["g4", "p1", "p2"]);
        assert!(!tree.rename("g4", "G4").unwrap());
    }

    #[test]
    fn a_node_needs_a_group_as_its_parent_and_a_new_identifier() {
        let mut tree = example();

        assert!(matches!(
            tree.insert(group("g5", Some("p1")), None),
            Err(TreeError::Refused(_))
        ));
        assert!(matches!(
            tree.insert(group("g5", Some("nothing")), None),
            Err(TreeError::NotFound(_))
        ));
        assert!(matches!(
            tree.insert(group("p1", None), None),
            Err(TreeError::Refused(_))
        ));
        // A list where a node comes before its parent is not a tree.
        assert!(
            WorkspaceTree::from_nodes([project("p1", Some("g1"), "/a"), group("g1", None)])
                .is_err()
        );
    }

    #[test]
    fn a_session_belongs_to_the_project_with_the_longest_directory_above_it() {
        let mut tree = example();
        tree.insert(project("p5", None, "/src/a"), None).unwrap();
        let project_of = |directory: &str| {
            tree.project_of(Path::new(directory))
                .map(|node| node.id.as_str())
        };

        assert_eq!(project_of("/src/a/web/src/lib"), Some("p2"));
        assert_eq!(project_of("/src/a/web"), Some("p2"));
        assert_eq!(project_of("/src/a/other"), Some("p5"));
        // Complete path components only.
        assert_eq!(project_of("/src/a/webapp"), Some("p5"));
        assert_eq!(project_of("/src/application"), None);
        assert_eq!(project_of("/home/user"), None);
    }
}
