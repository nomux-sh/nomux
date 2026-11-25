//! The JSON form of a report and of an error. `docs/cli.md` has the schema.

use nomux_client::{AttachEnd, ClientError, HostInfo, Session};
use serde::Serialize;

use super::extension_json::ExtensionBody;
use super::human::state_name;
use crate::exit_code;
use crate::report::{AddedHost, HostDiagnosis, Report};

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
    Sessions(Vec<SessionDocument<'a>>),
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
struct SessionDocument<'a> {
    id: &'a str,
    state: &'static str,
    pid: Option<u32>,
}

impl<'a> From<&'a Session> for SessionDocument<'a> {
    fn from(session: &'a Session) -> Self {
        Self {
            id: &session.id,
            state: state_name(session.state),
            pid: session.pid,
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
}

impl<'a> From<Option<&'a HostInfo>> for DaemonDocument<'a> {
    fn from(info: Option<&'a HostInfo>) -> Self {
        Self {
            running: info.is_some(),
            version: info.map(|info| info.daemon_version.as_str()),
            pid: info.map(|info| info.daemon_pid),
            sessions: info.map(|info| info.session_count),
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
        Report::SessionCreated(session) => Body::Session(session.into()),
        Report::Sessions(sessions) => {
            Body::Sessions(sessions.iter().map(SessionDocument::from).collect())
        }
        Report::SessionKilled { id } => Body::Killed(id),
        Report::AttachEnded { id, end } => Body::Attach(AttachDocument::new(id, *end)),
        Report::ClientsDetached { id, clients } => Body::Detached(DetachedDocument {
            id,
            clients: *clients,
        }),
        Report::DaemonStatus(info) => Body::Daemon(info.as_ref().into()),
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
        Report::Version => Body::Version(env!("CARGO_PKG_VERSION")),
    };
    line(body)
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
    use nomux_client::SessionState;

    use super::*;

    fn session(pid: Option<u32>) -> Session {
        Session {
            id: "ab".to_owned(),
            state: SessionState::Running,
            pid,
        }
    }

    #[test]
    fn session_documents() {
        assert_eq!(
            report(&Report::SessionCreated(session(Some(7)))),
            "{\"schema_version\":1,\"session\":{\"id\":\"ab\",\"state\":\"running\",\"pid\":7}}\n"
        );
        assert_eq!(
            report(&Report::Sessions(vec![session(None)])),
            "{\"schema_version\":1,\"sessions\":[{\"id\":\"ab\",\"state\":\"running\",\"pid\":null}]}\n"
        );
        assert_eq!(
            report(&Report::Sessions(Vec::new())),
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
            report(&Report::DaemonStatus(Some(info))),
            "{\"schema_version\":1,\"daemon\":{\"running\":true,\"version\":\"1.2.3\",\"pid\":7,\"sessions\":2}}\n"
        );
        assert_eq!(
            report(&Report::DaemonStatus(None)),
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
