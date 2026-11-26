//! The result of a command that did not fail.

use std::path::PathBuf;

use nomux_client::{
    AttachEnd, HostCheck, HostInfo, Installation, RestorePolicy, Session, SessionMemory,
};
use nomux_core::configuration::HostEntry;

use crate::exit_code;
use crate::extension_commands::ExtensionReport;

#[derive(Debug)]
pub(crate) enum Report {
    SessionCreated(Session),
    Sessions(Vec<Session>),
    /// The sessions of `ls --memory`.
    SessionsMeasured {
        sessions: Vec<Session>,
        /// One entry for each session that has a process.
        memory: Vec<SessionMemory>,
    },
    SessionStarted(Session),
    OutputCaptured {
        id: String,
        /// The output bytes, as the command of the session wrote them.
        output: Vec<u8>,
    },
    RestorePolicySet {
        id: String,
        policy: RestorePolicy,
    },
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
    HostAdded(AddedHost),
    Hosts(Vec<HostEntry>),
    HostRemoved {
        name: String,
    },
    HostsChecked(Vec<HostDiagnosis>),
    Extension(ExtensionReport),
    ServiceFile(ServiceFileReport),
    Version,
}

/// What a command did with the file that starts the daemon at login.
#[derive(Debug)]
pub(crate) struct ServiceFileReport {
    pub(crate) action: ServiceFileAction,
    pub(crate) path: PathBuf,
    /// The content of the file. Only for an install.
    pub(crate) content: Option<String>,
    /// The command that tells the service manager about the change.
    pub(crate) next_step: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceFileAction {
    Installed,
    /// A dry run of an install.
    WouldInstall,
    Removed,
    /// A dry run of a removal.
    WouldRemove,
    /// The file does not exist, thus there is nothing to remove.
    NotInstalled,
}

/// A remote host that `host add` saved.
#[derive(Debug)]
pub(crate) struct AddedHost {
    pub(crate) entry: HostEntry,
    pub(crate) installation: Installation,
    /// The daemon that answers on the host.
    pub(crate) daemon: HostInfo,
}

/// The checks of one host of `host doctor`.
#[derive(Debug)]
pub(crate) struct HostDiagnosis {
    pub(crate) name: String,
    pub(crate) checks: Vec<HostCheck>,
}

impl Report {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::DaemonStatus(None) => exit_code::NOT_FOUND,
            Self::AttachEnded { end, .. } => exit_code::of_attach_end(*end),
            Self::HostsChecked(hosts) => {
                let all_passed = hosts
                    .iter()
                    .flat_map(|host| &host.checks)
                    .all(|check| check.passed);
                if all_passed {
                    exit_code::SUCCESS
                } else {
                    exit_code::ERROR
                }
            }
            Self::Extension(report) => report.exit_code(),
            _ => exit_code::SUCCESS,
        }
    }

    /// The payload of the result of a method of an extension. It is bytes in
    /// the encoding of the method, not text.
    pub(crate) fn call_payload(&self) -> Option<&[u8]> {
        match self {
            Self::Extension(ExtensionReport::Called { payload, .. }) => Some(payload),
            _ => None,
        }
    }
}
