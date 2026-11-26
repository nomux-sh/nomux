//! The form of a status in the control protocol.

use nomux_holder_protocol::SessionId;
use nomux_proto::wire::v1 as wire;
use nomux_status::{
    AgentRun, ChildWork, NeedsYouKind, Source, SourceReport, Sourced, State, Status,
};

/// The Agent run of the session `id` with the `details` of its adapter.
/// Without `include_children`, the Agent run has no Child work, and its
/// aggregate still includes them.
pub(crate) fn agent_run(
    id: &SessionId,
    run: &AgentRun,
    details: Option<&wire::AgentDetails>,
    include_children: bool,
) -> wire::AgentRun {
    wire::AgentRun {
        session_id: id.to_string(),
        status: Some(status(&run.status)),
        aggregate: Some(status(&run.aggregate)),
        children: if include_children {
            run.children.iter().map(child_work).collect()
        } else {
            Vec::new()
        },
        details: details.cloned(),
    }
}

pub(crate) fn status(status: &Status) -> wire::AgentStatus {
    let (state, kind) = state(status.state);
    wire::AgentStatus {
        state: state.into(),
        kind: kind.into(),
        source: status
            .source
            .map_or(wire::StatusSource::None, source)
            .into(),
        since_ms: status.since.0,
        seen: status.seen,
        message: status.message.as_ref().map(text),
        title: status.title.as_ref().map(text),
        progress: status
            .progress
            .as_ref()
            .map(|progress| wire::StatusProgress {
                percent: progress.value.into(),
                source: source(progress.source).into(),
            }),
        tool: status.tool.as_ref().map(text),
    }
}

/// What one source says, for `Agent.Explain`.
pub(crate) fn source_report(report: &SourceReport) -> wire::SourceReport {
    let (state, kind) = report.root.as_ref().map_or(
        (
            wire::AgentState::Unspecified,
            wire::NeedsYouKind::Unspecified,
        ),
        |root| state(root.state),
    );
    wire::SourceReport {
        source: source(report.source).into(),
        reported: report.root.is_some(),
        state: state.into(),
        kind: kind.into(),
        message: report
            .root
            .as_ref()
            .and_then(|root| root.message.clone())
            .unwrap_or_default(),
        note: report.note.clone(),
    }
}

/// The state of a self-report. `None` for a value that is not a state.
/// The self-report does not say what work waits for.
pub(crate) fn reported_state(state: i32) -> Option<State> {
    reported_state_and_kind(state, wire::NeedsYouKind::Unspecified.into())
}

/// The state of a report with the kind of a state that needs the user.
/// `None` for a value that is not a state. A kind that the daemon does not
/// know is no kind.
pub(crate) fn reported_state_and_kind(state: i32, kind: i32) -> Option<State> {
    let kind = match wire::NeedsYouKind::try_from(kind) {
        Ok(wire::NeedsYouKind::Permission) => Some(NeedsYouKind::Permission),
        Ok(wire::NeedsYouKind::Question) => Some(NeedsYouKind::Question),
        Ok(wire::NeedsYouKind::Auth) => Some(NeedsYouKind::Auth),
        Ok(wire::NeedsYouKind::Prompt) => Some(NeedsYouKind::Prompt),
        Ok(wire::NeedsYouKind::Unspecified) | Err(_) => None,
    };
    match wire::AgentState::try_from(state).ok()? {
        wire::AgentState::Unspecified => None,
        wire::AgentState::Idle => Some(State::Idle),
        wire::AgentState::Working => Some(State::Working),
        wire::AgentState::Done => Some(State::Done),
        wire::AgentState::NeedsYou => Some(State::NeedsYou(kind)),
        wire::AgentState::Error => Some(State::Error),
    }
}

fn child_work(child: &ChildWork) -> wire::ChildWork {
    wire::ChildWork {
        name: child.name.clone(),
        status: child.status.as_ref().map(status),
        aggregate: Some(status(&child.aggregate)),
        children: child.children.iter().map(child_work).collect(),
    }
}

fn state(state: State) -> (wire::AgentState, wire::NeedsYouKind) {
    let kind = |kind: Option<NeedsYouKind>| match kind {
        None => wire::NeedsYouKind::Unspecified,
        Some(NeedsYouKind::Permission) => wire::NeedsYouKind::Permission,
        Some(NeedsYouKind::Question) => wire::NeedsYouKind::Question,
        Some(NeedsYouKind::Auth) => wire::NeedsYouKind::Auth,
        Some(NeedsYouKind::Prompt) => wire::NeedsYouKind::Prompt,
    };
    match state {
        State::Idle => (wire::AgentState::Idle, wire::NeedsYouKind::Unspecified),
        State::Working => (wire::AgentState::Working, wire::NeedsYouKind::Unspecified),
        State::Done => (wire::AgentState::Done, wire::NeedsYouKind::Unspecified),
        State::NeedsYou(needs) => (wire::AgentState::NeedsYou, kind(needs)),
        State::Error => (wire::AgentState::Error, wire::NeedsYouKind::Unspecified),
    }
}

fn source(source: Source) -> wire::StatusSource {
    match source {
        Source::Adapter => wire::StatusSource::Adapter,
        Source::ProgramStatus => wire::StatusSource::ProgramStatus,
        Source::SelfReport => wire::StatusSource::SelfReport,
        Source::SessionRestore => wire::StatusSource::SessionRestore,
        Source::PromptMarks => wire::StatusSource::PromptMarks,
        Source::ForegroundProgram => wire::StatusSource::ForegroundProgram,
    }
}

fn text(text: &Sourced<String>) -> wire::StatusText {
    wire::StatusText {
        text: text.value.clone(),
        source: source(text.source).into(),
    }
}
