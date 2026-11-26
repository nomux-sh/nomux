//! The command line of the status commands.

use clap::{Subcommand, ValueEnum};
use nomux_client::{EventType, ReportedState};

#[derive(Debug, Subcommand)]
pub(crate) enum StatusCommand {
    /// Report the status of the session of this process. The command works
    /// only inside a nomux session
    Set {
        /// The state of the work in the session. idle takes the report back
        state: StateArgument,

        /// What the work does, waits for or completed: one line
        message: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
    /// Show what each source says about the status of a session
    Explain {
        /// The session identifier
        id: String,
    },
}

/// A state that a program reports for its session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum StateArgument {
    Working,
    Done,
    Blocked,
    Error,
    Idle,
}

impl From<StateArgument> for ReportedState {
    fn from(state: StateArgument) -> Self {
        match state {
            StateArgument::Working => Self::Working,
            StateArgument::Done => Self::Done,
            StateArgument::Blocked => Self::Blocked,
            StateArgument::Error => Self::Error,
            StateArgument::Idle => Self::Idle,
        }
    }
}

/// A type of event for `events --filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum EventTypeArgument {
    /// The status of a session or of its Child work changed
    AgentRunChanged,
    /// A session ended or was removed
    AgentRunRemoved,
    /// The daemon part of an extension sent an event
    ExtensionEvent,
}

impl From<EventTypeArgument> for EventType {
    fn from(kind: EventTypeArgument) -> Self {
        match kind {
            EventTypeArgument::AgentRunChanged => Self::AgentRunChanged,
            EventTypeArgument::AgentRunRemoved => Self::AgentRunRemoved,
            EventTypeArgument::ExtensionEvent => Self::ExtensionEvent,
        }
    }
}
