//! The result of a status command that did not fail.

use nomux_client::{AgentExplanation, AgentTree, ReportedState, Session};

use crate::report::Report;

#[derive(Debug)]
pub(crate) enum StatusReport {
    /// The status of each session.
    Tree {
        tree: AgentTree,
        /// The sessions that are not ended, for their commands.
        sessions: Vec<Session>,
        /// `--tree`: show the Child work.
        with_children: bool,
    },
    Explained(AgentExplanation),
    Set {
        id: String,
        state: ReportedState,
    },
    /// `events --follow` printed each event when it came.
    EventsFollowed,
}

impl From<StatusReport> for Report {
    fn from(report: StatusReport) -> Self {
        Self::Status(report)
    }
}
