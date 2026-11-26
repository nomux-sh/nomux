//! The status of the Agent run of each session: the data of the Agent
//! service.

use nomux_proto::wire::v1 as wire;

/// What an Agent run or a Child work does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    /// Nothing runs, and nothing waits for the user.
    Idle,
    /// Work runs.
    Working,
    /// Work is complete.
    Done,
    /// The work cannot continue until the user does something. The kind is
    /// `None` if no source says what.
    NeedsYou(Option<NeedsYouKind>),
    /// The work failed and stopped.
    Error,
    /// A state that this client does not know.
    Unknown,
}

/// What the user must do for work that waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedsYouKind {
    /// Approve an operation.
    Permission,
    /// Type an answer.
    Question,
    /// Give a login, a token or a credential.
    Auth,
}

/// Where a part of a status comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSource {
    /// No source reports a status.
    None,
    /// An adapter extension reported it from the events of the agent tool,
    /// for example from its hooks.
    Adapter,
    /// A program reported it in the terminal stream (OSC 7501).
    ProgramStatus,
    /// A program in the session reported it with
    /// [`Client::set_agent_status`](crate::Client::set_agent_status).
    SelfReport,
    /// The session lost its process while work ran or waited.
    SessionRestore,
    /// A guess from the prompt marks of the shell (OSC 133).
    PromptMarks,
    /// A guess from the foreground program of the terminal.
    ForegroundProgram,
    /// A source that this client does not know.
    Unknown,
}

impl StatusSource {
    /// `true` if the daemon guessed the status and no program reported it.
    /// Show a guess as a guess, not as a fact.
    #[must_use]
    pub fn is_guess(self) -> bool {
        matches!(self, Self::PromptMarks | Self::ForegroundProgram)
    }
}

/// A value of a status and its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    /// The value.
    pub value: T,
    /// The source of the value.
    pub source: StatusSource,
}

/// The status of one Agent run or of one Child work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    /// What the work does.
    pub state: AgentState,
    /// The source of the state.
    pub source: StatusSource,
    /// When the daemon saw the state for the first time: milliseconds after
    /// the Unix epoch.
    pub since_ms: u64,
    /// `true` if the user saw the result of [`AgentState::Done`] or of
    /// [`AgentState::Error`].
    pub seen: bool,
    /// What the work does, waits for or completed.
    pub message: Option<Sourced<String>>,
    /// The label of the work.
    pub title: Option<Sourced<String>>,
    /// The percentage of the work that is done.
    pub progress: Option<Sourced<u32>>,
    /// The name of the tool that runs.
    pub tool: Option<Sourced<String>>,
}

/// One Child work below an Agent run or below a Child work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildWork {
    /// The name of the Child work below its parent.
    pub name: String,
    /// `None` if no source reports this Child work: only a Child work below
    /// it has a status.
    pub status: Option<AgentStatus>,
    /// The most urgent status of this Child work and of each one below it.
    pub aggregate: AgentStatus,
    /// The Child work below this one, in the sequence of their names.
    pub children: Vec<ChildWork>,
}

/// What an adapter extension says about the Agent run of a session,
/// beyond its status. The source is always [`StatusSource::Adapter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDetails {
    /// The model that the agent uses.
    pub model: Option<String>,
    /// The identifier of the session of the agent tool.
    pub provider_session_id: Option<String>,
    /// A command that continues the session of the agent tool.
    pub resume_command: Option<String>,
}

/// The status of what runs in one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    /// The identifier of the session.
    pub session_id: String,
    /// The status of the Agent run.
    pub status: AgentStatus,
    /// The most urgent status of the Agent run and of each Child work below
    /// it.
    pub aggregate: AgentStatus,
    /// The Child work of the Agent run, in the sequence of their names.
    pub children: Vec<ChildWork>,
    /// What the adapter of the agent tool says about the Agent run. `None`
    /// if no adapter gave details.
    pub details: Option<AgentDetails>,
}

/// The status of each session of a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTree {
    /// One Agent run for each session that is not ended, in the sequence of
    /// the session identifiers.
    pub runs: Vec<AgentRun>,
    /// The session with the most urgent status, and that status. `None` if
    /// the host has no session.
    pub attention: Option<(String, AgentStatus)>,
}

/// The most urgent of `statuses`, by the aggregation rule of the status
/// engine: work that needs the user first, then an error and a result that
/// the user did not see, then work that runs. With the same urgency, the
/// status that waits longest wins, then the first one. `None` if there is
/// no status.
pub fn most_urgent<'status>(
    statuses: impl IntoIterator<Item = &'status AgentStatus>,
) -> Option<&'status AgentStatus> {
    let ranked: Vec<(&AgentStatus, nomux_status::Status)> = statuses
        .into_iter()
        .map(|status| (status, engine_status(status)))
        .collect();
    nomux_status::most_urgent(ranked.iter().map(|(status, ranked)| (*status, ranked)))
        .map(|(status, _)| status)
}

/// The parts of `status` that the aggregation rule compares, as the status
/// engine has them.
fn engine_status(status: &AgentStatus) -> nomux_status::Status {
    let state = match status.state {
        AgentState::Idle | AgentState::Unknown => nomux_status::State::Idle,
        AgentState::Working => nomux_status::State::Working,
        AgentState::Done => nomux_status::State::Done,
        AgentState::NeedsYou(_) => nomux_status::State::NeedsYou(None),
        AgentState::Error => nomux_status::State::Error,
    };
    nomux_status::Status {
        state,
        seen: status.seen,
        ..nomux_status::Status::unreported(nomux_status::Timestamp(status.since_ms))
    }
}

/// What one source says about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceExplanation {
    /// The source.
    pub source: StatusSource,
    /// The state that the source reports. `None` if it reports nothing.
    pub state: Option<AgentState>,
    /// The message that the source reports.
    pub message: Option<String>,
    /// Why the source reports nothing, or what its report is based on: a
    /// text in English for a person.
    pub note: String,
}

/// Why a session has its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExplanation {
    /// The status of the session.
    pub run: AgentRun,
    /// Each source, in the sequence in which they decide.
    pub sources: Vec<SourceExplanation>,
}

/// A state that a program in a session reports with
/// [`Client::set_agent_status`](crate::Client::set_agent_status).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedState {
    /// Work runs.
    Working,
    /// Work is complete.
    Done,
    /// The work waits for the user.
    Blocked,
    /// The work failed and stopped.
    Error,
    /// Takes the self-report back.
    Idle,
}

impl From<ReportedState> for wire::AgentState {
    fn from(state: ReportedState) -> Self {
        match state {
            ReportedState::Working => Self::Working,
            ReportedState::Done => Self::Done,
            ReportedState::Blocked => Self::NeedsYou,
            ReportedState::Error => Self::Error,
            ReportedState::Idle => Self::Idle,
        }
    }
}

pub(crate) fn state_of(state: i32, kind: i32) -> AgentState {
    match wire::AgentState::try_from(state) {
        Ok(wire::AgentState::Idle) => AgentState::Idle,
        Ok(wire::AgentState::Working) => AgentState::Working,
        Ok(wire::AgentState::Done) => AgentState::Done,
        Ok(wire::AgentState::NeedsYou) => AgentState::NeedsYou(needs_you_kind(kind)),
        Ok(wire::AgentState::Error) => AgentState::Error,
        Ok(wire::AgentState::Unspecified) | Err(_) => AgentState::Unknown,
    }
}

fn needs_you_kind(kind: i32) -> Option<NeedsYouKind> {
    match wire::NeedsYouKind::try_from(kind).ok()? {
        wire::NeedsYouKind::Permission => Some(NeedsYouKind::Permission),
        wire::NeedsYouKind::Question => Some(NeedsYouKind::Question),
        wire::NeedsYouKind::Auth => Some(NeedsYouKind::Auth),
        // A kind that this client does not show yet.
        wire::NeedsYouKind::Prompt | wire::NeedsYouKind::Unspecified => None,
    }
}

fn source_of(source: i32) -> StatusSource {
    match wire::StatusSource::try_from(source) {
        Ok(wire::StatusSource::None) => StatusSource::None,
        Ok(wire::StatusSource::Adapter) => StatusSource::Adapter,
        Ok(wire::StatusSource::ProgramStatus) => StatusSource::ProgramStatus,
        Ok(wire::StatusSource::SelfReport) => StatusSource::SelfReport,
        Ok(wire::StatusSource::SessionRestore) => StatusSource::SessionRestore,
        Ok(wire::StatusSource::PromptMarks) => StatusSource::PromptMarks,
        Ok(wire::StatusSource::ForegroundProgram) => StatusSource::ForegroundProgram,
        Ok(wire::StatusSource::Unspecified) | Err(_) => StatusSource::Unknown,
    }
}

fn text(text: wire::StatusText) -> Sourced<String> {
    Sourced {
        value: text.text,
        source: source_of(text.source),
    }
}

/// A status that the daemon left out has no source and no state.
pub(crate) fn status_or_unknown(status: Option<wire::AgentStatus>) -> AgentStatus {
    status.unwrap_or_default().into()
}

impl From<wire::AgentStatus> for AgentStatus {
    fn from(status: wire::AgentStatus) -> Self {
        Self {
            state: state_of(status.state, status.kind),
            source: source_of(status.source),
            since_ms: status.since_ms,
            seen: status.seen,
            message: status.message.map(text),
            title: status.title.map(text),
            progress: status.progress.map(|progress| Sourced {
                value: progress.percent,
                source: source_of(progress.source),
            }),
            tool: status.tool.map(text),
        }
    }
}

impl From<wire::ChildWork> for ChildWork {
    fn from(child: wire::ChildWork) -> Self {
        Self {
            name: child.name,
            status: child.status.map(AgentStatus::from),
            aggregate: status_or_unknown(child.aggregate),
            children: child.children.into_iter().map(Self::from).collect(),
        }
    }
}

impl From<wire::AgentRun> for AgentRun {
    fn from(run: wire::AgentRun) -> Self {
        Self {
            session_id: run.session_id,
            status: status_or_unknown(run.status),
            aggregate: status_or_unknown(run.aggregate),
            children: run.children.into_iter().map(ChildWork::from).collect(),
            details: run.details.map(AgentDetails::from),
        }
    }
}

impl From<wire::AgentDetails> for AgentDetails {
    fn from(details: wire::AgentDetails) -> Self {
        let text = |text: String| (!text.is_empty()).then_some(text);
        Self {
            model: text(details.model),
            provider_session_id: text(details.provider_session_id),
            resume_command: text(details.resume_command),
        }
    }
}

impl From<wire::AgentTreeResponse> for AgentTree {
    fn from(tree: wire::AgentTreeResponse) -> Self {
        let attention = tree
            .aggregate
            .filter(|_| !tree.attention_session_id.is_empty())
            .map(|status| (tree.attention_session_id, status.into()));
        Self {
            runs: tree.runs.into_iter().map(AgentRun::from).collect(),
            attention,
        }
    }
}

impl From<wire::SourceReport> for SourceExplanation {
    fn from(report: wire::SourceReport) -> Self {
        Self {
            source: source_of(report.source),
            state: report.reported.then(|| state_of(report.state, report.kind)),
            message: (!report.message.is_empty()).then_some(report.message),
            note: report.note,
        }
    }
}
