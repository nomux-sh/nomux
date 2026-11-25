//! The JSON form of a report and of an error. `docs/cli.md` has the schema.

use nomux_client::{ClientError, HostInfo, Session};
use serde::Serialize;

use super::human::state_name;
use crate::exit_code;
use crate::report::Report;

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
    Daemon(DaemonDocument<'a>),
    Stopped(bool),
    Version(&'a str),
    Error(ErrorDocument<'a>),
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
        Report::DaemonStatus(info) => Body::Daemon(info.as_ref().into()),
        Report::DaemonStopped { was_running } => Body::Stopped(*was_running),
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
    fn error_document() {
        let not_found = ClientError::NotFound("no session".to_owned());
        assert_eq!(
            error(&not_found, exit_code::of_error(&not_found)),
            "{\"schema_version\":1,\"error\":{\"code\":2,\"kind\":\"not_found\",\"message\":\"no session\"}}\n"
        );
    }
}
