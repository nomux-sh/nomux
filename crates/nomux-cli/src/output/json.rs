//! The JSON form of a report and of an error. `docs/cli.md` has the schema.

use std::borrow::Cow;
use std::path::Path;

use nomux_client::{
    AttachEnd, ClientError, ExitStatus, HostEvent, HostInfo, RestorePolicy, Session, SessionExit,
};
use serde::Serialize;

use super::extension_json::ExtensionBody;
use super::hooks_json::HooksBody;
use super::human::state_name;
use super::memory_json::SessionsMemoryBody;
use super::status_json::StatusBody;
use super::workspace_json::{ProjectDocument, WorkspaceBody};
use crate::exit_code;
use crate::report::{AddedHost, HostDiagnosis, Report, ServiceFileAction, ServiceFileReport};

/// The version of the JSON schema. It increases when a field is removed or
/// changes its meaning. A new field does not increase it.
const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
struct Document<'a> {
    schema_version: u32,
    #[serde(flatten)]
    body: Body<'a>,
}

/// Each variant is one top-level field of the document.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Body<'a> {
    Session(SessionDocument<'a>),
    Sessions(Vec<ListedSessionDocument<'a>>),
    Capture(CaptureDocument<'a>),
    RestorePolicy(RestorePolicyDocument<'a>),
    Service(ServiceDocument<'a>),
    Killed(&'a str),
    Attach(AttachDocument<'a>),
    Detached(DetachedDocument<'a>),
    Daemon(DaemonDocument<'a>),
    Stopped(bool),
    Host(AddedHostDocument<'a>),
    Hosts(Vec<HostDocument<'a>>),
    Removed(&'a str),
    Doctor(Vec<DiagnosisDocument<'a>>),
    Version(&'a str),
    Error(ErrorDocument<'a>),
    /// The field of the document is the field of the body of the extension.
    #[serde(untagged)]
    Extension(ExtensionBody<'a>),
    /// The fields of the document are the fields of the body.
    #[serde(untagged)]
    SessionsMemory(SessionsMemoryBody<'a>),
    /// The field of the document is the field of the body of the status
    /// command.
    #[serde(untagged)]
    Status(Box<StatusBody<'a>>),
    /// The field of the document is the field of the body of the hook
    /// command.
    #[serde(untagged)]
    Hooks(HooksBody<'a>),
    /// The field of the document is the field of the body of the
    /// `project` or `group` command.
    #[serde(untagged)]
    Workspace(WorkspaceBody<'a>),
}

/// A session of `ls`, with its Project.
#[derive(Serialize)]
struct ListedSessionDocument<'a> {
    #[serde(flatten)]
    session: SessionDocument<'a>,
    project: Option<ProjectDocument<'a>>,
}

#[derive(Serialize)]
struct HostDocument<'a> {
    name: &'a str,
    ssh: &'a str,
}

#[derive(Serialize)]
struct AddedHostDocument<'a> {
    name: &'a str,
    ssh: &'a str,
    system: &'a str,
    machine: &'a str,
    target: &'a str,
    directory: String,
    installed: bool,
    daemon: DaemonDocument<'a>,
}

impl<'a> From<&'a AddedHost> for AddedHostDocument<'a> {
    fn from(added: &'a AddedHost) -> Self {
        let installation = &added.installation;
        Self {
            name: &added.entry.name,
            ssh: &added.entry.ssh,
            system: &installation.system,
            machine: &installation.machine,
            target: &installation.target,
            directory: format!("~/.nomux/bin/{}", installation.label),
            installed: installation.installed_now,
            daemon: Some(&added.daemon).into(),
        }
    }
}

#[derive(Serialize)]
struct DiagnosisDocument<'a> {
    host: &'a str,
    ok: bool,
    checks: Vec<CheckDocument<'a>>,
}

#[derive(Serialize)]
struct CheckDocument<'a> {
    name: &'a str,
    ok: bool,
    detail: &'a str,
}

impl<'a> From<&'a HostDiagnosis> for DiagnosisDocument<'a> {
    fn from(host: &'a HostDiagnosis) -> Self {
        Self {
            host: &host.name,
            ok: host.checks.iter().all(|check| check.passed),
            checks: host
                .checks
                .iter()
                .map(|check| CheckDocument {
                    name: check.name,
                    ok: check.passed,
                    detail: &check.detail,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
pub(super) struct SessionDocument<'a> {
    id: &'a str,
    state: &'static str,
    pid: Option<u32>,
    command: &'a [String],
    cwd: Option<&'a Path>,
    rows: Option<u16>,
    columns: Option<u16>,
    started_at: Option<u64>,
    epoch: Option<u32>,
    restore_policy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit: Option<ExitDocument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    damage: Option<&'a str>,
}

impl<'a> From<&'a Session> for SessionDocument<'a> {
    fn from(session: &'a Session) -> Self {
        Self {
            id: &session.id,
            state: state_name(session.state),
            pid: session.pid,
            command: &session.command,
            cwd: session.working_directory.as_deref(),
            rows: session.size.map(|size| size.rows),
            columns: session.size.map(|size| size.columns),
            started_at: session.started_at,
            epoch: session.epoch,
            restore_policy: session.restore_policy.map(RestorePolicy::as_str),
            exit: session.exit.map(ExitDocument::from),
            damage: session.damage.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct ExitDocument {
    ended_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signal: Option<i32>,
}

impl From<SessionExit> for ExitDocument {
    fn from(exit: SessionExit) -> Self {
        let (exit_code, signal) = match exit.status {
            ExitStatus::Exited { code } => (Some(code), None),
            ExitStatus::Signaled { signal } => (None, Some(signal)),
            ExitStatus::Unknown => (None, None),
        };
        Self {
            ended_at: exit.ended_at,
            exit_code,
            signal,
        }
    }
}

#[derive(Serialize)]
struct CaptureDocument<'a> {
    id: &'a str,
    /// A byte sequence that is not UTF-8 has U+FFFD in its place.
    output: Cow<'a, str>,
}

#[derive(Serialize)]
struct RestorePolicyDocument<'a> {
    id: &'a str,
    policy: &'static str,
}

#[derive(Serialize)]
struct ServiceDocument<'a> {
    action: &'static str,
    path: &'a Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
}

impl<'a> From<&'a ServiceFileReport> for ServiceDocument<'a> {
    fn from(service: &'a ServiceFileReport) -> Self {
        let action = match service.action {
            ServiceFileAction::Installed => "installed",
            ServiceFileAction::WouldInstall => "would_install",
            ServiceFileAction::Removed => "removed",
            ServiceFileAction::WouldRemove => "would_remove",
            ServiceFileAction::NotInstalled => "not_installed",
        };
        Self {
            action,
            path: &service.path,
            content: service.content.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct AttachDocument<'a> {
    id: &'a str,
    end: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signal: Option<i32>,
}

impl<'a> AttachDocument<'a> {
    fn new(id: &'a str, end: AttachEnd) -> Self {
        let (end, exit_code, signal) = match end {
            AttachEnd::Detached => ("detached", None, None),
            AttachEnd::Exited { code } => ("exited", Some(code), None),
            AttachEnd::Signaled { signal } => ("signaled", None, Some(signal)),
            AttachEnd::Lost => ("lost", None, None),
            AttachEnd::Unknown => ("unknown", None, None),
        };
        Self {
            id,
            end,
            exit_code,
            signal,
        }
    }
}

#[derive(Serialize)]
struct DetachedDocument<'a> {
    id: &'a str,
    clients: u32,
}

#[derive(Serialize)]
struct DaemonDocument<'a> {
    running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sessions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    configuration: Option<&'a Path>,
}

impl<'a> From<Option<&'a HostInfo>> for DaemonDocument<'a> {
    fn from(info: Option<&'a HostInfo>) -> Self {
        Self {
            running: info.is_some(),
            version: info.map(|info| info.daemon_version.as_str()),
            pid: info.map(|info| info.daemon_pid),
            sessions: info.map(|info| info.session_count),
            configuration: None,
        }
    }
}

#[derive(Serialize)]
struct ErrorDocument<'a> {
    code: u8,
    kind: &'static str,
    message: &'a str,
}

pub(super) fn report(report: &Report) -> String {
    let body = match report {
        Report::SessionCreated(session) | Report::SessionStarted(session) => {
            Body::Session(session.into())
        }
        Report::OutputCaptured { id, output } => Body::Capture(CaptureDocument {
            id,
            output: String::from_utf8_lossy(output),
        }),
        Report::RestorePolicySet { id, policy } => Body::RestorePolicy(RestorePolicyDocument {
            id,
            policy: policy.as_str(),
        }),
        Report::ServiceFile(service) => Body::Service(service.into()),
        Report::Sessions {
            sessions,
            workspace,
        } => Body::Sessions(
            sessions
                .iter()
                .map(|session| ListedSessionDocument {
                    session: session.into(),
                    project: ProjectDocument::of_session(session, &workspace.tree),
                })
                .collect(),
        ),
        Report::Workspace(report) => Body::Workspace(WorkspaceBody::of(report)),
        Report::SessionsMeasured { sessions, memory } => {
            Body::SessionsMemory(SessionsMemoryBody::new(sessions, memory))
        }
        Report::SessionKilled { id } => Body::Killed(id),
        Report::AttachEnded { id, end } => Body::Attach(AttachDocument::new(id, *end)),
        Report::ClientsDetached { id, clients } => Body::Detached(DetachedDocument {
            id,
            clients: *clients,
        }),
        Report::DaemonStatus {
            info,
            configuration,
        } => Body::Daemon(DaemonDocument {
            configuration: configuration.as_deref(),
            ..info.as_ref().into()
        }),
        Report::DaemonStopped { was_running } => Body::Stopped(*was_running),
        Report::HostAdded(added) => Body::Host(added.into()),
        Report::Hosts(hosts) => Body::Hosts(
            hosts
                .iter()
                .map(|host| HostDocument {
                    name: &host.name,
                    ssh: &host.ssh,
                })
                .collect(),
        ),
        Report::HostRemoved { name } => Body::Removed(name),
        Report::HostsChecked(hosts) => {
            Body::Doctor(hosts.iter().map(DiagnosisDocument::from).collect())
        }
        Report::Extension(report) => Body::Extension(report.into()),
        Report::Status(report) => match StatusBody::of(report) {
            Some(body) => Body::Status(Box::new(body)),
            None => return String::new(),
        },
        Report::Hooks(report) => match HooksBody::of(report) {
            Some(body) => Body::Hooks(body),
            None => return String::new(),
        },
        Report::Version => Body::Version(env!("CARGO_PKG_VERSION")),
    };
    line(body)
}

/// One line of `events --follow --json`: one document for each event.
pub(super) fn event(event: &HostEvent) -> String {
    line(Body::Status(Box::new(StatusBody::event(event))))
}

pub(super) fn error(error: &ClientError, code: u8) -> String {
    let message = error.to_string();
    line(Body::Error(ErrorDocument {
        code,
        kind: exit_code::name(code),
        message: &message,
    }))
}

fn line(body: Body<'_>) -> String {
    let document = Document {
        schema_version: SCHEMA_VERSION,
        body,
    };
    let mut text = serde_json::to_string(&document)
        .expect("a document of strings, numbers and booleans is valid JSON");
    text.push('\n');
    text
}

/// The documents are a contract with scripts. Each test has the full text,
/// thus a changed field name or a removed field fails it.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nomux_client::{SessionState, WindowSize, Workspace};

    use super::*;

    /// A session whose description the daemon does not know.
    fn session(pid: Option<u32>) -> Session {
        Session {
            id: "ab".to_owned(),
            state: SessionState::Running,
            pid,
            command: Vec::new(),
            working_directory: None,
            size: None,
            started_at: None,
            epoch: None,
            restore_policy: None,
            exit: None,
            damage: None,
            resolved_working_directory: None,
        }
    }

    #[test]
    fn session_documents() {
        let described = Session {
            command: vec!["vim".to_owned(), "a b".to_owned()],
            working_directory: Some(PathBuf::from("/home/user")),
            size: Some(WindowSize {
                rows: 24,
                columns: 80,
            }),
            started_at: Some(5),
            epoch: Some(2),
            restore_policy: Some(RestorePolicy::OnFocus),
            ..session(Some(7))
        };
        assert_eq!(
            report(&Report::SessionCreated(described)),
            "{\"schema_version\":1,\"session\":{\"id\":\"ab\",\"state\":\"running\",\"pid\":7,\"command\":[\"vim\",\"a b\"],\"cwd\":\"/home/user\",\"rows\":24,\"columns\":80,\"started_at\":5,\"epoch\":2,\"restore_policy\":\"on-focus\"}}\n"
        );
        assert_eq!(
            report(&Report::Sessions {
                sessions: vec![session(None)],
                workspace: Workspace::default(),
            }),
            "{\"schema_version\":1,\"sessions\":[{\"id\":\"ab\",\"state\":\"running\",\"pid\":null,\"command\":[],\"cwd\":null,\"rows\":null,\"columns\":null,\"started_at\":null,\"epoch\":null,\"restore_policy\":null,\"project\":null}]}\n"
        );
        let ended = Session {
            state: SessionState::Ended,
            exit: Some(SessionExit {
                ended_at: 9,
                status: ExitStatus::Signaled { signal: 15 },
            }),
            damage: Some("a reason".to_owned()),
            ..session(None)
        };
        assert_eq!(
            report(&Report::SessionStarted(ended)),
            "{\"schema_version\":1,\"session\":{\"id\":\"ab\",\"state\":\"ended\",\"pid\":null,\"command\":[],\"cwd\":null,\"rows\":null,\"columns\":null,\"started_at\":null,\"epoch\":null,\"restore_policy\":null,\"exit\":{\"ended_at\":9,\"signal\":15},\"damage\":\"a reason\"}}\n"
        );
        assert_eq!(
            report(&Report::Sessions {
                sessions: Vec::new(),
                workspace: Workspace::default(),
            }),
            "{\"schema_version\":1,\"sessions\":[]}\n"
        );
        assert_eq!(
            report(&Report::SessionKilled {
                id: "ab".to_owned()
            }),
            "{\"schema_version\":1,\"killed\":\"ab\"}\n"
        );
    }

    #[test]
    fn capture_restore_policy_and_service_documents() {
        assert_eq!(
            report(&Report::OutputCaptured {
                id: "ab".to_owned(),
                output: b"a\x1b[0m\xff\n".to_vec(),
            }),
            "{\"schema_version\":1,\"capture\":{\"id\":\"ab\",\"output\":\"a\\u001b[0m\u{fffd}\\n\"}}\n"
        );
        assert_eq!(
            report(&Report::RestorePolicySet {
                id: "ab".to_owned(),
                policy: RestorePolicy::Manual,
            }),
            "{\"schema_version\":1,\"restore_policy\":{\"id\":\"ab\",\"policy\":\"manual\"}}\n"
        );
        let service = |action, content: Option<&str>| {
            report(&Report::ServiceFile(ServiceFileReport {
                action,
                path: PathBuf::from("/c/nomuxd.service"),
                content: content.map(str::to_owned),
                next_step: Some("a command".to_owned()),
            }))
        };
        assert_eq!(
            service(ServiceFileAction::WouldInstall, Some("[Unit]\n")),
            "{\"schema_version\":1,\"service\":{\"action\":\"would_install\",\"path\":\"/c/nomuxd.service\",\"content\":\"[Unit]\\n\"}}\n"
        );
        assert_eq!(
            service(ServiceFileAction::Removed, None),
            "{\"schema_version\":1,\"service\":{\"action\":\"removed\",\"path\":\"/c/nomuxd.service\"}}\n"
        );
    }

    #[test]
    fn attach_and_detach_documents() {
        let ended = |end| {
            report(&Report::AttachEnded {
                id: "ab".to_owned(),
                end,
            })
        };
        assert_eq!(
            ended(AttachEnd::Detached),
            "{\"schema_version\":1,\"attach\":{\"id\":\"ab\",\"end\":\"detached\"}}\n"
        );
        assert_eq!(
            ended(AttachEnd::Exited { code: 3 }),
            "{\"schema_version\":1,\"attach\":{\"id\":\"ab\",\"end\":\"exited\",\"exit_code\":3}}\n"
        );
        assert_eq!(
            ended(AttachEnd::Signaled { signal: 9 }),
            "{\"schema_version\":1,\"attach\":{\"id\":\"ab\",\"end\":\"signaled\",\"signal\":9}}\n"
        );
        assert_eq!(
            ended(AttachEnd::Lost),
            "{\"schema_version\":1,\"attach\":{\"id\":\"ab\",\"end\":\"lost\"}}\n"
        );
        assert_eq!(
            report(&Report::ClientsDetached {
                id: "ab".to_owned(),
                clients: 2
            }),
            "{\"schema_version\":1,\"detached\":{\"id\":\"ab\",\"clients\":2}}\n"
        );
    }

    #[test]
    fn daemon_documents() {
        let info = HostInfo {
            daemon_version: "1.2.3".to_owned(),
            daemon_pid: 7,
            session_count: 2,
        };
        assert_eq!(
            report(&Report::DaemonStatus {
                info: Some(info),
                configuration: Some("/home/user/.config/nomux/config.toml".into()),
            }),
            "{\"schema_version\":1,\"daemon\":{\"running\":true,\"version\":\"1.2.3\",\"pid\":7,\"sessions\":2,\"configuration\":\"/home/user/.config/nomux/config.toml\"}}\n"
        );
        assert_eq!(
            report(&Report::DaemonStatus {
                info: None,
                configuration: None,
            }),
            "{\"schema_version\":1,\"daemon\":{\"running\":false}}\n"
        );
        assert_eq!(
            report(&Report::DaemonStopped { was_running: true }),
            "{\"schema_version\":1,\"stopped\":true}\n"
        );
    }

    #[test]
    fn host_documents() {
        use nomux_client::{HostCheck, Installation};
        use nomux_core::configuration::HostEntry;

        let entry = HostEntry {
            name: "vps".to_owned(),
            ssh: "deploy@vps.example.com".to_owned(),
        };
        let added = AddedHost {
            entry: entry.clone(),
            installation: Installation {
                system: "Linux".to_owned(),
                machine: "x86_64".to_owned(),
                target: "x86_64-unknown-linux-musl".to_owned(),
                label: "1.2.3".to_owned(),
                daemon_socket: "/home/deploy/.nomux/run/nomuxd.sock".to_owned(),
                installed_now: true,
            },
            daemon: HostInfo {
                daemon_version: "1.2.3".to_owned(),
                daemon_pid: 7,
                session_count: 0,
            },
        };
        assert_eq!(
            report(&Report::HostAdded(added)),
            "{\"schema_version\":1,\"host\":{\"name\":\"vps\",\"ssh\":\"deploy@vps.example.com\",\"system\":\"Linux\",\"machine\":\"x86_64\",\"target\":\"x86_64-unknown-linux-musl\",\"directory\":\"~/.nomux/bin/1.2.3\",\"installed\":true,\"daemon\":{\"running\":true,\"version\":\"1.2.3\",\"pid\":7,\"sessions\":0}}}\n"
        );
        assert_eq!(
            report(&Report::Hosts(vec![entry])),
            "{\"schema_version\":1,\"hosts\":[{\"name\":\"vps\",\"ssh\":\"deploy@vps.example.com\"}]}\n"
        );
        assert_eq!(
            report(&Report::HostRemoved {
                name: "vps".to_owned()
            }),
            "{\"schema_version\":1,\"removed\":\"vps\"}\n"
        );
        let checked = Report::HostsChecked(vec![HostDiagnosis {
            name: "vps".to_owned(),
            checks: vec![
                HostCheck {
                    name: "connection",
                    passed: true,
                    detail: "deploy@vps port 22".to_owned(),
                },
                HostCheck {
                    name: "permissions",
                    passed: false,
                    detail: "wrong mode".to_owned(),
                },
            ],
        }]);
        assert_eq!(checked.exit_code(), exit_code::ERROR);
        assert_eq!(
            report(&checked),
            "{\"schema_version\":1,\"doctor\":[{\"host\":\"vps\",\"ok\":false,\"checks\":[{\"name\":\"connection\",\"ok\":true,\"detail\":\"deploy@vps port 22\"},{\"name\":\"permissions\",\"ok\":false,\"detail\":\"wrong mode\"}]}]}\n"
        );
    }

    #[test]
    fn error_document() {
        let not_found = ClientError::NotFound("no session".to_owned());
        assert_eq!(
            error(&not_found, exit_code::of_error(&not_found)),
            "{\"schema_version\":1,\"error\":{\"code\":2,\"kind\":\"not_found\",\"message\":\"no session\"}}\n"
        );
    }
}
