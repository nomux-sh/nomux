//! The JSON form of the report of a status command and of an event.
//! `docs/cli.md` has the schema.

use nomux_client::{
    AgentExplanation, AgentRun, AgentState, AgentStatus, AgentTree, ChildWork, EventBody,
    HostEvent, NeedsYouKind, ReportedState, Session, SourceExplanation, Sourced, StatusSource,
};
use serde::Serialize;

use crate::status_commands::StatusReport;

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
            } => Self::Status(TreeDocument::new(tree, sessions, *with_children)),
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
}

impl<'a> TreeDocument<'a> {
    fn new(tree: &'a AgentTree, sessions: &'a [Session], with_children: bool) -> Self {
        let command = |id: &str| {
            sessions
                .iter()
                .find(|session| session.id == id)
                .map_or(&[][..], |session| session.command.as_slice())
        };
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
                .map(|run| RunDocument::new(run, Some(command(&run.session_id)), with_children))
                .collect(),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<&'a [String]>,
    status: StatusDocument<'a>,
    aggregate: StatusDocument<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<ChildDocument<'a>>>,
}

impl<'a> RunDocument<'a> {
    fn new(run: &'a AgentRun, command: Option<&'a [String]>, with_children: bool) -> Self {
        Self {
            id: &run.session_id,
            command,
            status: (&run.status).into(),
            aggregate: (&run.aggregate).into(),
            children: with_children.then(|| run.children.iter().map(ChildDocument::from).collect()),
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
}

impl<'a> From<&'a HostEvent> for EventDocument<'a> {
    fn from(event: &'a HostEvent) -> Self {
        let (kind, session, run) = match &event.body {
            EventBody::AgentRunChanged(run) => (
                "agent_run_changed",
                Some(run.session_id.as_str()),
                Some(RunDocument::new(run, None, true)),
            ),
            EventBody::AgentRunRemoved { session_id } => {
                ("agent_run_removed", Some(session_id.as_str()), None)
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
