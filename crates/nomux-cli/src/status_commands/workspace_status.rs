//! The status of the Groups and the Projects of `status --tree`: each
//! session below the Project of its working directory, and the most urgent
//! status of the sessions below each node.

use std::collections::HashMap;

use nomux_client::{AgentRun, AgentStatus, Node, Session, WorkspaceTree, most_urgent};

use crate::workspace_commands::node_path::depth_of;

/// One node of the workspace tree with its status.
pub(crate) struct NodeStatus<'a> {
    pub(crate) node: &'a Node,
    /// The number of Groups above the node.
    pub(crate) depth: usize,
    /// The sessions of a Project. Empty for a Group.
    pub(crate) runs: Vec<&'a AgentRun>,
    /// The most urgent status of the sessions below the node. `None` if no
    /// session is below it.
    pub(crate) aggregate: Option<&'a AgentStatus>,
}

/// The sessions arranged in the workspace tree.
pub(crate) struct WorkspaceStatus<'a> {
    /// The sessions without a Project.
    pub(crate) outside: Vec<&'a AgentRun>,
    /// Each node, in the sequence of a depth-first walk.
    pub(crate) nodes: Vec<NodeStatus<'a>>,
}

impl<'a> WorkspaceStatus<'a> {
    pub(crate) fn new(tree: &'a WorkspaceTree, runs: &'a [AgentRun], sessions: &[Session]) -> Self {
        let mut outside = Vec::new();
        let mut by_project: HashMap<&str, Vec<&AgentRun>> = HashMap::new();
        // The statuses below each node: a session counts for its Project and
        // for each Group above it, in the sequence of the sessions.
        let mut below: HashMap<&str, Vec<&AgentStatus>> = HashMap::new();
        for run in runs {
            let project = sessions
                .iter()
                .find(|session| session.id == run.session_id)
                .and_then(|session| session.project(tree));
            let Some(project) = project else {
                outside.push(run);
                continue;
            };
            by_project.entry(&project.id).or_default().push(run);
            let mut current = Some(project);
            while let Some(node) = current {
                below.entry(&node.id).or_default().push(&run.aggregate);
                current = node
                    .parent_id
                    .as_deref()
                    .and_then(|parent| tree.get(parent));
            }
        }
        let nodes = tree
            .nodes()
            .into_iter()
            .map(|node| NodeStatus {
                node,
                depth: depth_of(tree, node),
                runs: by_project.remove(node.id.as_str()).unwrap_or_default(),
                aggregate: below
                    .get(node.id.as_str())
                    .and_then(|statuses| most_urgent(statuses.iter().copied())),
            })
            .collect();
        Self { outside, nodes }
    }
}

#[cfg(test)]
mod tests {
    use nomux_client::{AgentState, NodeKind, SessionState, StatusSource};

    use super::*;

    fn node(id: &str, parent_id: Option<&str>, directory: Option<&str>) -> Node {
        Node {
            id: id.to_owned(),
            parent_id: parent_id.map(str::to_owned),
            name: id.to_owned(),
            kind: directory.map_or(NodeKind::Group, |directory| NodeKind::Project {
                directory: directory.into(),
            }),
        }
    }

    fn session(id: &str, directory: &str) -> Session {
        Session {
            id: id.to_owned(),
            state: SessionState::Running,
            pid: None,
            command: Vec::new(),
            working_directory: Some(directory.into()),
            size: None,
            started_at: None,
            epoch: None,
            restore_policy: None,
            exit: None,
            damage: None,
            resolved_working_directory: None,
        }
    }

    fn run(id: &str, state: AgentState, since_ms: u64) -> AgentRun {
        let status = AgentStatus {
            state,
            source: StatusSource::ProgramStatus,
            since_ms,
            seen: false,
            message: None,
            title: None,
            progress: None,
            tool: None,
        };
        AgentRun {
            session_id: id.to_owned(),
            status: status.clone(),
            aggregate: status,
            children: Vec::new(),
            details: None,
        }
    }

    #[test]
    fn a_group_has_the_most_urgent_status_of_the_sessions_below_it() {
        let tree = WorkspaceTree::from_nodes([
            node("client", None, None),
            node("acme", Some("client"), None),
            node("web", Some("acme"), Some("/src/web")),
            node("api", Some("client"), Some("/src/api")),
            node("empty", None, None),
        ])
        .unwrap();
        let sessions = [
            session("s1", "/src/web/frontend"),
            session("s2", "/src/api"),
            session("s3", "/tmp"),
        ];
        let runs = [
            run("s1", AgentState::Working, 10),
            run("s2", AgentState::NeedsYou(None), 20),
            run("s3", AgentState::Error, 5),
        ];

        let status = WorkspaceStatus::new(&tree, &runs, &sessions);

        let outside: Vec<&str> = status
            .outside
            .iter()
            .map(|run| run.session_id.as_str())
            .collect();
        assert_eq!(outside, ["s3"]);
        let rows: Vec<(&str, usize, Vec<&str>, Option<AgentState>)> = status
            .nodes
            .iter()
            .map(|node| {
                (
                    node.node.id.as_str(),
                    node.depth,
                    node.runs
                        .iter()
                        .map(|run| run.session_id.as_str())
                        .collect(),
                    node.aggregate.map(|aggregate| aggregate.state),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("client", 0, vec![], Some(AgentState::NeedsYou(None))),
                ("acme", 1, vec![], Some(AgentState::Working)),
                ("web", 2, vec!["s1"], Some(AgentState::Working)),
                ("api", 1, vec!["s2"], Some(AgentState::NeedsYou(None))),
                ("empty", 0, vec![], None),
            ]
        );
    }
}
