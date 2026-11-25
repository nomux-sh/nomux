//! The text form of a report.

use std::env;
use std::fmt::Write;
use std::io::{self, IsTerminal};

use nomux_client::{AttachEnd, HostInfo, Session, SessionState};
use nomux_core::configuration::HostEntry;

use crate::report::{AddedHost, HostDiagnosis, Report};

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

/// If the text has colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Style {
    Plain,
    Colour,
}

impl Style {
    /// Colour only when a person reads the output on a terminal and did not
    /// set `NO_COLOR` (<https://no-color.org>).
    pub(super) fn of_standard_output() -> Self {
        let no_colour = env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        if io::stdout().is_terminal() && !no_colour {
            Self::Colour
        } else {
            Self::Plain
        }
    }
}

pub(super) fn report(report: &Report, style: Style) -> String {
    match report {
        Report::SessionCreated(session) => format!("{}\n", session.id),
        Report::Sessions(sessions) if sessions.is_empty() => "no sessions\n".to_owned(),
        Report::Sessions(sessions) => session_table(sessions, style),
        Report::SessionKilled { id } => format!("session {id} ended\n"),
        Report::AttachEnded { id, end } => attach_end(id, *end),
        Report::ClientsDetached { id, clients: 1 } => {
            format!("detached 1 client from session {id}\n")
        }
        Report::ClientsDetached { id, clients } => {
            format!("detached {clients} clients from session {id}\n")
        }
        Report::DaemonStatus(Some(info)) => daemon_status(info),
        Report::DaemonStatus(None) | Report::DaemonStopped { was_running: false } => {
            "nomuxd does not run\n".to_owned()
        }
        Report::DaemonStopped { was_running: true } => "nomuxd stopped\n".to_owned(),
        Report::HostAdded(added) => host_added(added),
        Report::Hosts(hosts) if hosts.is_empty() => "no hosts\n".to_owned(),
        Report::Hosts(hosts) => host_table(hosts),
        Report::HostRemoved { name } => format!("host {name} removed\n"),
        Report::HostsChecked(hosts) if hosts.is_empty() => "no hosts\n".to_owned(),
        Report::HostsChecked(hosts) => hosts.iter().map(host_diagnosis).collect(),
        Report::Version => format!("{}\n", nomux_core::version_line!()),
    }
}

/// Only what a script needs: identifiers, or nothing when the exit status
/// is the result.
pub(super) fn quiet_report(report: &Report) -> String {
    match report {
        Report::SessionCreated(session) => format!("{}\n", session.id),
        Report::Sessions(sessions) => sessions
            .iter()
            .flat_map(|session| [session.id.as_str(), "\n"])
            .collect(),
        Report::Version => format!("{}\n", env!("CARGO_PKG_VERSION")),
        Report::Hosts(hosts) => hosts
            .iter()
            .flat_map(|host| [host.name.as_str(), "\n"])
            .collect(),
        Report::SessionKilled { .. }
        | Report::AttachEnded { .. }
        | Report::ClientsDetached { .. }
        | Report::DaemonStatus(_)
        | Report::DaemonStopped { .. }
        | Report::HostAdded(_)
        | Report::HostRemoved { .. }
        | Report::HostsChecked(_) => String::new(),
    }
}

fn host_added(added: &AddedHost) -> String {
    let installation = &added.installation;
    let binaries = if installation.installed_now {
        "installed"
    } else {
        "already there"
    };
    format!(
        "host {} added: {} {}, nomux in ~/.nomux/bin/{} ({binaries}), {}",
        added.entry.name,
        installation.system,
        installation.machine,
        installation.label,
        daemon_status(&added.daemon)
    )
}

fn host_table(hosts: &[HostEntry]) -> String {
    const NAME: &str = "NAME";
    let name_width = column_width(NAME, hosts.iter().map(|host| host.name.as_str()));
    let mut table = format!("{NAME:<name_width$}  SSH\n");
    for host in hosts {
        // A write to a `String` does not fail.
        let _ = writeln!(table, "{:<name_width$}  {}", host.name, host.ssh);
    }
    table
}

/// The name of the host, then one line for each check: `ok` or `FAIL`, the
/// name of the check and what it found.
fn host_diagnosis(host: &HostDiagnosis) -> String {
    let name_width = column_width("", host.checks.iter().map(|check| check.name));
    let mut text = format!("{}\n", host.name);
    for check in &host.checks {
        let result = if check.passed { "ok" } else { "FAIL" };
        let _ = writeln!(
            text,
            "  {result:<4}  {:<name_width$}  {}",
            check.name, check.detail
        );
    }
    text
}

fn attach_end(id: &str, end: AttachEnd) -> String {
    match end {
        AttachEnd::Detached => format!("detached from session {id}\n"),
        AttachEnd::Exited { code } => format!("session {id} ended with exit status {code}\n"),
        AttachEnd::Signaled { signal } => format!("session {id} ended by signal {signal}\n"),
        AttachEnd::Lost | AttachEnd::Unknown => {
            format!("session {id} ended; its exit status is not known\n")
        }
    }
}

fn daemon_status(info: &HostInfo) -> String {
    let sessions = match info.session_count {
        1 => "1 session".to_owned(),
        count => format!("{count} sessions"),
    };
    format!(
        "nomuxd {} runs: process ID {}, {sessions}\n",
        info.daemon_version, info.daemon_pid
    )
}

fn session_table(sessions: &[Session], style: Style) -> String {
    const ID: &str = "ID";
    const STATE: &str = "STATE";
    const PID: &str = "PID";

    let rows: Vec<(&str, SessionState, String)> = sessions
        .iter()
        .map(|session| {
            let pid = session
                .pid
                .map_or_else(|| "-".to_owned(), |pid| pid.to_string());
            (session.id.as_str(), session.state, pid)
        })
        .collect();
    let id_width = column_width(ID, rows.iter().map(|row| row.0));
    let state_width = column_width(STATE, rows.iter().map(|row| state_name(row.1)));

    let mut table = format!("{ID:<id_width$}  {STATE:<state_width$}  {PID}\n");
    for (id, state, pid) in rows {
        let state_text = format!("{:<state_width$}", state_name(state));
        let state_text = coloured(&state_text, state, style);
        // A write to a `String` does not fail.
        let _ = writeln!(table, "{id:<id_width$}  {state_text}  {pid}");
    }
    table
}

fn column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.map(str::len).fold(header.len(), usize::max)
}

pub(super) fn state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Running => "running",
        SessionState::Unreachable => "unreachable",
        SessionState::Unknown => "unknown",
    }
}

/// The colour comes after the padding, thus the columns stay aligned.
fn coloured(padded: &str, state: SessionState, style: Style) -> String {
    let colour = match (style, state) {
        (Style::Plain, _) | (Style::Colour, SessionState::Unknown) => return padded.to_owned(),
        (Style::Colour, SessionState::Running) => GREEN,
        (Style::Colour, SessionState::Unreachable) => YELLOW,
    };
    format!("{colour}{padded}{RESET}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessions() -> Vec<Session> {
        vec![
            Session {
                id: "ab".to_owned(),
                state: SessionState::Running,
                pid: Some(7),
            },
            Session {
                id: "a-long-identifier".to_owned(),
                state: SessionState::Unreachable,
                pid: None,
            },
        ]
    }

    #[test]
    fn the_columns_are_aligned_to_the_longest_value() {
        let table = session_table(&sessions(), Style::Plain);

        assert_eq!(
            table,
            "ID                 STATE        PID\n\
             ab                 running      7\n\
             a-long-identifier  unreachable  -\n"
        );
    }

    #[test]
    fn each_check_of_a_host_is_one_line_with_its_result() {
        use nomux_client::HostCheck;

        let diagnosis = HostDiagnosis {
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
                    detail: "~/.nomux/run has the mode drwxr-xr-x".to_owned(),
                },
            ],
        };

        assert_eq!(
            host_diagnosis(&diagnosis),
            "vps\n  ok    connection   deploy@vps port 22\n  FAIL  permissions  ~/.nomux/run has the mode drwxr-xr-x\n"
        );
    }

    #[test]
    fn colour_does_not_change_the_alignment() {
        let coloured = session_table(&sessions(), Style::Colour);

        assert!(coloured.contains(GREEN));
        let without_colour = [GREEN, YELLOW, RESET]
            .iter()
            .fold(coloured, |text, code| text.replace(code, ""));
        assert_eq!(without_colour, session_table(&sessions(), Style::Plain));
    }
}
