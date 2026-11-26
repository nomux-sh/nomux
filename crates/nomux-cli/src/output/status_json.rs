//! The JSON form of the report of a status command and of an event.
//! `docs/cli.md` has the schema.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nomux_client::{
    AgentDetails, AgentExplanation, AgentRun, AgentState, AgentStatus, AgentTree, ChildWork,
    EventBody, HostEvent, NeedsYouKind, ReportedState, Session, SourceExplanation, Sourced,
    StatusSource, WorkspaceChange, WorkspaceTree,
};
use serde::Serialize;

use super::workspace_json::NodeDocument;
use crate::status_commands::{StatusReport, WorkspaceStatus};

/// Each variant is one top-level field of the document.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StatusBody<'a> {
    Status(TreeDocument<'a>),
    Explain(ExplainDocument<'a>),
    StatusSet(StatusSetDocument<'a>),
    Event(EventDocument<'a>),
}

impl<'a> StatusBody<'a> {
    /// `None` for a report that has no document: `events --follow` printed
    /// one document for each event.
    pub(super) fn of(report: &'a StatusReport) -> Option<Self> {
        Some(match report {
            StatusReport::Tree {
                tree,
                sessions,
                with_children,
                workspace,
            } => Self::Status(TreeDocument::new(
                tree,
                sessions,
                *with_children,
                workspace.as_ref().map(|workspace| &workspace.tree),
            )),
            StatusReport::Explained(explanation) => Self::Explain(explanation.into()),
            StatusReport::Set { id, state } => Self::StatusSet(StatusSetDocument {
                session: id,
                state: reported_name(*state),
            }),
            StatusReport::EventsFollowed => return None,
        })
    }

    pub(super) fn event(event: &'a HostEvent) -> Self {
        Self::Event(event.into())
    }
}

#[derive(Serialize)]
pub(super) struct TreeDocument<'a> {
    /// The session with the most urgent status.
    attention: Option<AttentionDocument<'a>>,
    sessions: Vec<RunDocument<'a>>,
    /// `--tree`: each node of the workspace tree with its status.
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<Vec<NodeStatusDocument<'a>>>,
}

/// A node of the workspace tree with the most urgent status of the sessions
/// below it, and the sessions of a Project.
#[derive(Serialize)]
struct NodeStatusDocument<'a> {
    #[serde(flatten)]
    node: NodeDocument<'a>,
    aggregate: Option<StatusDocument<'a>>,
    sessions: Vec<&'a str>,
}

impl<'a> TreeDocument<'a> {
    fn new(
        tree: &'a AgentTree,
        sessions: &'a [Session],
        with_children: bool,
        workspace: Option<&'a WorkspaceTree>,
    ) -> Self {
        let session_of = |id: &str| sessions.iter().find(|session| session.id == id);
        Self {
            attention: tree
                .attention
                .as_ref()
                .map(|(session, status)| AttentionDocument {
                    session,
                    status: status.into(),
                }),
            sessions: tree
                .runs
                .iter()
                .map(|run| {
                    RunDocument::new(
                        run,
                        Some(RunSessionDocument::of(session_of(&run.session_id))),
                        with_children,
                    )
                })
                .collect(),
            workspace: workspace.map(|workspace| {
                WorkspaceStatus::new(workspace, &tree.runs, sessions)
                    .nodes
                    .into_iter()
                    .map(|status| NodeStatusDocument {
                        node: NodeDocument::of(status.node, workspace),
                        aggregate: status.aggregate.map(StatusDocument::from),
                        sessions: status
                            .runs
                            .iter()
                            .map(|run| run.session_id.as_str())
                            .collect(),
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Serialize)]
struct AttentionDocument<'a> {
    session: &'a str,
    status: StatusDocument<'a>,
}

#[derive(Serialize)]
struct RunDocument<'a> {
    id: &'a str,
    /// Only in the tree.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    session: Option<RunSessionDocument<'a>>,
    status: StatusDocument<'a>,
    aggregate: StatusDocument<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<ChildDocument<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<DetailsDocument<'a>>,
}

/// The session of an Agent run in the tree.
#[derive(Serialize)]
struct RunSessionDocument<'a> {
    name: Option<&'a str>,
    command: &'a [String],
}

impl<'a> RunSessionDocument<'a> {
    /// `None`: the session of the run is not in the list.
    fn of(session: Option<&'a Session>) -> Self {
        Self {
            name: session.and_then(|session| session.name.as_deref()),
            command: session.map_or(&[][..], |session| &session.command),
        }
    }
}

/// What the adapter of the agent tool says about the Agent run.
#[derive(Serialize)]
struct DetailsDocument<'a> {
    model: Option<&'a str>,
    provider_session_id: Option<&'a str>,
    resume_command: Option<&'a str>,
}

impl<'a> From<&'a AgentDetails> for DetailsDocument<'a> {
    fn from(details: &'a AgentDetails) -> Self {
        Self {
            model: details.model.as_deref(),
            provider_session_id: details.provider_session_id.as_deref(),
            resume_command: details.resume_command.as_deref(),
        }
    }
}

impl<'a> RunDocument<'a> {
    /// `session` is the session of the run in the tree, `None` elsewhere.
    fn new(
        run: &'a AgentRun,
        session: Option<RunSessionDocument<'a>>,
        with_children: bool,
    ) -> Self {
        Self {
            id: &run.session_id,
            session,
            status: (&run.status).into(),
            aggregate: (&run.aggregate).into(),
            children: with_children.then(|| run.children.iter().map(ChildDocument::from).collect()),
            details: run.details.as_ref().map(DetailsDocument::from),
        }
    }
}

#[derive(Serialize)]
struct ChildDocument<'a> {
    name: &'a str,
    status: Option<StatusDocument<'a>>,
    aggregate: StatusDocument<'a>,
    children: Vec<ChildDocument<'a>>,
}

impl<'a> From<&'a ChildWork> for ChildDocument<'a> {
    fn from(child: &'a ChildWork) -> Self {
        Self {
            name: &child.name,
            status: child.status.as_ref().map(StatusDocument::from),
            aggregate: (&child.aggregate).into(),
            children: child.children.iter().map(Self::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct StatusDocument<'a> {
    state: &'static str,
    kind: Option<&'static str>,
    source: &'static str,
    guess: bool,
    since_ms: u64,
    seen: bool,
    message: Option<TextDocument<'a>>,
    title: Option<TextDocument<'a>>,
    progress: Option<ProgressDocument>,
    tool: Option<TextDocument<'a>>,
}

impl<'a> From<&'a AgentStatus> for StatusDocument<'a> {
    fn from(status: &'a AgentStatus) -> Self {
        Self {
            state: state_name(status.state),
            kind: kind_name(status.state),
            source: source_name(status.source),
            guess: status.source.is_guess(),
            since_ms: status.since_ms,
            seen: status.seen,
            message: status.message.as_ref().map(TextDocument::from),
            title: status.title.as_ref().map(TextDocument::from),
            progress: status.progress.as_ref().map(|progress| ProgressDocument {
                percent: progress.value,
                source: source_name(progress.source),
            }),
            tool: status.tool.as_ref().map(TextDocument::from),
        }
    }
}

#[derive(Serialize)]
struct TextDocument<'a> {
    text: &'a str,
    source: &'static str,
}

impl<'a> From<&'a Sourced<String>> for TextDocument<'a> {
    fn from(text: &'a Sourced<String>) -> Self {
        Self {
            text: &text.value,
            source: source_name(text.source),
        }
    }
}

#[derive(Serialize)]
struct ProgressDocument {
    percent: u32,
    source: &'static str,
}

#[derive(Serialize)]
pub(super) struct ExplainDocument<'a> {
    session: &'a str,
    run: RunDocument<'a>,
    sources: Vec<SourceDocument<'a>>,
}

impl<'a> From<&'a AgentExplanation> for ExplainDocument<'a> {
    fn from(explanation: &'a AgentExplanation) -> Self {
        Self {
            session: &explanation.run.session_id,
            run: RunDocument::new(&explanation.run, None, true),
            sources: explanation
                .sources
                .iter()
                .map(SourceDocument::from)
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct SourceDocument<'a> {
    source: &'static str,
    guess: bool,
    reported: bool,
    state: Option<&'static str>,
    kind: Option<&'static str>,
    message: Option<&'a str>,
    note: &'a str,
}

impl<'a> From<&'a SourceExplanation> for SourceDocument<'a> {
    fn from(source: &'a SourceExplanation) -> Self {
        Self {
            source: source_name(source.source),
            guess: source.source.is_guess(),
            reported: source.state.is_some(),
            state: source.state.map(state_name),
            kind: source.state.and_then(kind_name),
            message: source.message.as_deref(),
            note: &source.note,
        }
    }
}

#[derive(Serialize)]
pub(super) struct StatusSetDocument<'a> {
    session: &'a str,
    state: &'static str,
}

#[derive(Serialize)]
pub(super) struct EventDocument<'a> {
    seq: u64,
    at_ms: u64,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<RunDocument<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<ExtensionEventDocument<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<Box<WorkspaceEventDocument<'a>>>,
}

/// A change of the workspace tree.
#[derive(Serialize)]
struct WorkspaceEventDocument<'a> {
    revision: u64,
    #[serde(flatten)]
    change: ChangeDocument<'a>,
}

#[derive(Serialize)]
#[serde(tag = "change", rename_all = "snake_case")]
enum ChangeDocument<'a> {
    Added {
        node: NodeDocument<'a>,
        index: usize,
    },
    Renamed {
        id: &'a str,
        name: &'a str,
    },
    Moved {
        id: &'a str,
        parent_id: Option<&'a str>,
        index: usize,
    },
    Removed {
        id: &'a str,
    },
    Unknown,
}

impl<'a> From<&'a WorkspaceChange> for ChangeDocument<'a> {
    fn from(change: &'a WorkspaceChange) -> Self {
        match change {
            WorkspaceChange::Added { node, index } => Self::Added {
                node: NodeDocument::without_path(node),
                index: *index,
            },
            WorkspaceChange::Renamed { id, name } => Self::Renamed { id, name },
            WorkspaceChange::Moved {
                id,
                parent_id,
                index,
            } => Self::Moved {
                id,
                parent_id: parent_id.as_deref(),
                index: *index,
            },
            WorkspaceChange::Removed { id } => Self::Removed { id },
            WorkspaceChange::Unknown => Self::Unknown,
        }
    }
}

/// An event of an extension. The payload is base64: the extension selects
/// its encoding, and it is not always text.
#[derive(Serialize)]
struct ExtensionEventDocument<'a> {
    id: &'a str,
    name: &'a str,
    payload_base64: String,
}

impl<'a> From<&'a HostEvent> for EventDocument<'a> {
    fn from(event: &'a HostEvent) -> Self {
        let mut extension = None;
        let mut workspace = None;
        let (kind, session, run) = match &event.body {
            EventBody::AgentRunChanged(run) => (
                "agent_run_changed",
                Some(run.session_id.as_str()),
                Some(RunDocument::new(run, None, true)),
            ),
            EventBody::AgentRunRemoved { session_id } => {
                ("agent_run_removed", Some(session_id.as_str()), None)
            }
            EventBody::ExtensionEvent {
                extension_id,
                name,
                payload,
            } => {
                extension = Some(ExtensionEventDocument {
                    id: extension_id,
                    name,
                    payload_base64: STANDARD.encode(payload),
                });
                ("extension_event", None, None)
            }
            EventBody::WorkspaceChanged { revision, change } => {
                workspace = Some(Box::new(WorkspaceEventDocument {
                    revision: *revision,
                    change: change.into(),
                }));
                ("workspace_changed", None, None)
            }
            EventBody::Snapshot => ("snapshot", None, None),
            EventBody::Unknown => ("unknown", None, None),
        };
        Self {
            seq: event.seq,
            at_ms: event.at_ms,
            kind,
            session,
            run,
            extension,
            workspace,
        }
    }
}

fn state_name(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "idle",
        AgentState::Working => "working",
        AgentState::Done => "done",
        AgentState::NeedsYou(_) => "needs_you",
        AgentState::Error => "error",
        AgentState::Unknown => "unknown",
    }
}

fn kind_name(state: AgentState) -> Option<&'static str> {
    match state {
        AgentState::NeedsYou(Some(NeedsYouKind::Permission)) => Some("permission"),
        AgentState::NeedsYou(Some(NeedsYouKind::Question)) => Some("question"),
        AgentState::NeedsYou(Some(NeedsYouKind::Auth)) => Some("auth"),
        _ => None,
    }
}

fn source_name(source: StatusSource) -> &'static str {
    match source {
        StatusSource::None => "none",
        StatusSource::Adapter => "adapter",
        StatusSource::ProgramStatus => "program_status",
        StatusSource::SelfReport => "self_report",
        StatusSource::SessionRestore => "session_restore",
        StatusSource::PromptMarks => "prompt_marks",
        StatusSource::ForegroundProgram => "foreground_program",
        StatusSource::Unknown => "unknown",
    }
}

fn reported_name(state: ReportedState) -> &'static str {
    match state {
        ReportedState::Working => "working",
        ReportedState::Done => "done",
        ReportedState::Blocked => "needs_you",
        ReportedState::Error => "error",
        ReportedState::Idle => "idle",
    }
}
