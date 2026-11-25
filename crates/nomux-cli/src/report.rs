//! The result of a command that did not fail.

use nomux_client::{AttachEnd, HostInfo, Session};

use crate::exit_code;

#[derive(Debug)]
pub(crate) enum Report {
    SessionCreated(Session),
    Sessions(Vec<Session>),
    SessionKilled {
        id: String,
    },
    AttachEnded {
        id: String,
        end: AttachEnd,
    },
    ClientsDetached {
        id: String,
        /// The number of clients that were attached to the session.
        clients: u32,
    },
    /// `None`: no daemon runs.
    DaemonStatus(Option<HostInfo>),
    DaemonStopped {
        /// `false`: no daemon ran, thus the command had nothing to stop.
        was_running: bool,
    },
    Version,
}

impl Report {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::DaemonStatus(None) => exit_code::NOT_FOUND,
            Self::AttachEnded { end, .. } => exit_code::of_attach_end(*end),
            _ => exit_code::SUCCESS,
        }
    }
}
