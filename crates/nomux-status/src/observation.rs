//! What one source says at one moment: the input of the resolver.

use crate::source::Source;
use crate::state::State;

/// What a source says about one Agent run or one Child work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// What the work does.
    pub state: State,
    /// What the work does, waits for or completed.
    pub message: Option<String>,
    /// The label of the work.
    pub title: Option<String>,
    /// The percentage of the work that is done: 0 to 100.
    pub progress: Option<u8>,
    /// The name of the tool that runs.
    pub tool: Option<String>,
}

impl Observation {
    /// An observation that has only a state.
    #[must_use]
    pub fn of(state: State) -> Self {
        Self {
            state,
            message: None,
            title: None,
            progress: None,
            tool: None,
        }
    }
}

/// What a source says about one Child work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildObservation {
    /// The names from the Agent run down to the Child work. A path that is
    /// empty or that has more than [`MAX_DEPTH`](crate::MAX_DEPTH) names is
    /// ignored.
    pub path: Vec<String>,
    /// What the source says about the Child work.
    pub observation: Observation,
}

/// What one source says about the Agent run of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceReport {
    /// The source.
    pub source: Source,
    /// What the source says about the Agent run. `None` if the source
    /// reports no status for it.
    pub root: Option<Observation>,
    /// What the source says about the Child work.
    pub children: Vec<ChildObservation>,
    /// Why the source reports nothing, or what its report is based on: a
    /// text in English for a person.
    pub note: String,
}

impl SourceReport {
    /// A report of a source that says nothing.
    #[must_use]
    pub fn silent(source: Source, note: impl Into<String>) -> Self {
        Self {
            source,
            root: None,
            children: Vec::new(),
            note: note.into(),
        }
    }
}
