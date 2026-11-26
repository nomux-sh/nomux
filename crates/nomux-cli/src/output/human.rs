//! The text form of a report.

use std::env;
use std::fmt::Write;
use std::io::{self, IsTerminal};

use nomux_client::{
    AttachEnd, ExitStatus, HibernationReason, HostInfo, Session, SessionState, WorkspaceTree,
};
use nomux_core::configuration::HostEntry;

use super::memory_size::memory_size;
use super::table::{Cell, Table};
use super::{
    extension_human, hooks_human, memory_human, status_human, workspace_human,
    worktree_clean_human, worktree_human,
};
use crate::report::{AddedHost, HostDiagnosis, Report, ServiceFileAction, ServiceFileReport};
use crate::workspace_commands::node_path::path_of;

pub(super) const RED: &str = "\x1b[31m";
pub(super) const GREEN: &str = "\x1b[32m";
pub(super) const YELLOW: &str = "\x1b[33m";
pub(super) const BLUE: &str = "\x1b[34m";
pub(super) const RESET: &str = "\x1b[0m";
const NOT_RUNNING: &str = "nomuxd does not run\n";

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

/// The text of `report`. A table fits `width` columns: the width of the
/// terminal, or `None` for no limit.
pub(super) fn report(report: &Report, style: Style, width: Option<usize>) -> String {
    match report {
        Report::SessionCreated(session) => format!("{}\n", session.id),
        Report::Sessions { sessions, .. } if sessions.is_empty() => "no sessions\n".to_owned(),
        Report::Sessions {
            sessions,
            workspace,
        } => session_table(sessions, &workspace.tree, style, width),
        Report::SessionsMeasured { sessions, .. } if sessions.is_empty() => {
            "no sessions\n".to_owned()
        }
        Report::SessionsMeasured { sessions, memory } => {
            memory_human::session_table(sessions, memory, width)
        }
        Report::SessionStarted(session) => format!("session {} runs\n", described(session)),
        Report::SessionHibernated(session) => hibernated(session),
        Report::SessionWoken(session) => format!(
            "session {} woke: {}\n",
            described(session),
            session
                .hibernation
                .as_ref()
                .map_or_default(|hibernation| hibernation.resume_command.as_str())
        ),
        Report::SessionPinned {
            session,
            pinned: true,
        } => format!("session {} is pinned\n", described(session)),
        Report::SessionPinned {
            session,
            pinned: false,
        } => format!("session {} is not pinned\n", described(session)),
        Report::OutputCaptured { output, .. } => String::from_utf8_lossy(output).into_owned(),
        Report::RestorePolicySet { session, policy } => {
            format!(
                "session {} has the restore policy {policy}\n",
                described(session)
            )
        }
        Report::SessionKilled { session } => format!("session {} ended\n", described(session)),
        Report::AttachEnded { session, end, .. } => attach_end(&described(session), *end),
        Report::ClientsDetached {
            session,
            clients: 1,
        } => {
            format!("detached 1 client from session {}\n", described(session))
        }
        Report::ClientsDetached { session, clients } => {
            format!(
                "detached {clients} clients from session {}\n",
                described(session)
            )
        }
        Report::SessionsKilled { sessions, .. } if sessions.is_empty() => {
            "no session runs\n".to_owned()
        }
        Report::SessionsKilled {
            sessions, dry_run, ..
        } => {
            let verb = if *dry_run { "would end" } else { "ended" };
            sessions.iter().fold(String::new(), |mut text, session| {
                let _ = writeln!(text, "session {} {verb}", described(session));
                text
            })
        }
        Report::Sent { .. } | Report::Followed => String::new(),
        Report::SessionRenamed { session, .. } => format!(
            "session {} is now named {}\n",
            session.id,
            session.name.as_deref().unwrap_or("-")
        ),
        Report::DaemonStatus {
            info,
            configuration,
        } => {
            let mut text = info
                .as_ref()
                .map_or_else(|| NOT_RUNNING.to_owned(), daemon_status);
            if let Some(path) = configuration {
                let _ = writeln!(text, "configuration file: {}", path.display());
            }
            text
        }
        Report::DaemonStopped { was_running: false } => NOT_RUNNING.to_owned(),
        Report::DaemonStopped { was_running: true } => "nomuxd stopped\n".to_owned(),
        Report::HostAdded(added) => host_added(added),
        Report::Hosts(hosts) if hosts.is_empty() => "no hosts\n".to_owned(),
        Report::Hosts(hosts) => host_table(hosts, width),
        Report::HostRemoved { name } => format!("host {name} removed\n"),
        Report::HostsChecked(hosts) if hosts.is_empty() => "no hosts\n".to_owned(),
        Report::HostsChecked(hosts) => hosts.iter().map(host_diagnosis).collect(),
        Report::Extension(report) => extension_human::report(report, width),
        Report::Hooks(report) => hooks_human::report(report, width),
        Report::Status(report) => status_human::report(report, style, width),
        Report::Workspace(report) => workspace_human::report(report, width),
        Report::Worktrees(listing) => worktree_human::report(listing, width),
        Report::Worktree(report) => worktree_clean_human::report(report, width),
        Report::ServiceFile(service) => service_file(service),
        Report::Version => format!("{}\n", nomux_core::version_line!()),
    }
}

/// Only what a script needs: identifiers, or nothing when the exit status
/// is the result.
pub(super) fn quiet_report(report: &Report) -> String {
    match report {
        Report::SessionCreated(session) => format!("{}\n", session.id),
        Report::Sessions { sessions, .. } | Report::SessionsMeasured { sessions, .. } => sessions
            .iter()
            .flat_map(|session| [session.id.as_str(), "\n"])
            .collect(),
        Report::Version => format!("{}\n", env!("CARGO_PKG_VERSION")),
        Report::Hosts(hosts) => hosts
            .iter()
            .flat_map(|host| [host.name.as_str(), "\n"])
            .collect(),
        Report::Extension(report) => extension_human::quiet_report(report),
        Report::Hooks(report) => hooks_human::quiet_report(report),
        Report::Status(report) => status_human::quiet_report(report),
        Report::Workspace(report) => workspace_human::quiet_report(report),
        Report::Worktrees(listing) => worktree_human::quiet_report(listing),
        Report::Worktree(report) => worktree_clean_human::quiet_report(report),
        Report::OutputCaptured { output, .. } => String::from_utf8_lossy(output).into_owned(),
        Report::SessionStarted(_)
        | Report::SessionHibernated(_)
        | Report::SessionWoken(_)
        | Report::SessionPinned { .. }
        | Report::RestorePolicySet { .. }
        | Report::ServiceFile(_)
        | Report::SessionKilled { .. }
        | Report::AttachEnded { .. }
        | Report::ClientsDetached { .. }
        | Report::SessionRenamed { .. }
        | Report::SessionsKilled { .. }
        | Report::Sent { .. }
        | Report::Followed
        | Report::DaemonStatus { .. }
        | Report::DaemonStopped { .. }
        | Report::HostAdded(_)
        | Report::HostRemoved { .. }
        | Report::HostsChecked(_) => String::new(),
    }
}

fn hibernated(session: &Session) -> String {
    let freed = session
        .hibernation
        .as_ref()
        .and_then(|hibernation| hibernation.freed)
        .map_or_else(
            || "the memory was not measured".to_owned(),
            |(bytes, measure)| {
                format!(
                    "{} given back ({})",
                    memory_size(bytes),
                    memory_human::measure_name(measure)
                )
            },
        );
    format!("session {} hibernated: {freed}\n", session.id)
}

fn service_file(service: &ServiceFileReport) -> String {
    let path = service.path.display();
    let next_step = service.next_step.as_deref().unwrap_or_default();
    match service.action {
        ServiceFileAction::Installed => {
            format!("wrote {path}\nTo start the daemon at each login, run: {next_step}\n")
        }
        ServiceFileAction::WouldInstall => format!(
            "# {path}\n{}",
            service.content.as_deref().unwrap_or_default()
        ),
        ServiceFileAction::Removed => format!(
            "removed {path}\nThe daemon that runs continues. To make the service manager \
             forget the file, run: {next_step}\n"
        ),
        ServiceFileAction::WouldRemove => format!("would remove {path}\n"),
        ServiceFileAction::NotInstalled => {
            format!("no service file: {path} does not exist\n")
        }
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

fn host_table(hosts: &[HostEntry], width: Option<usize>) -> String {
    let mut table = Table::new(&["NAME", "SSH"]).shorten(&[1]);
    for host in hosts {
        table.push(vec![Cell::new(&host.name), Cell::new(&host.ssh)]);
    }
    table.render(width)
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

/// The name and the identifier of a session, for a message: `api
/// (k3xq2abc)`, or the identifier of a session without a name.
fn described(session: &Session) -> String {
    match &session.name {
        Some(name) => format!("{name} ({})", session.id),
        None => session.id.clone(),
    }
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
    let mut text = format!(
        "nomuxd {} runs: process ID {}, {sessions}\n",
        info.daemon_version, info.daemon_pid
    );
    if let Some(limit) = &info.memory_limit {
        let _ = writeln!(text, "memory limit: {}", limit.detail);
    }
    text
}

/// One row of the session table.
struct SessionRow<'a> {
    id: &'a str,
    name: Option<&'a str>,
    state: SessionState,
    hibernated: bool,
    pid: String,
    /// The path of the Project of the session.
    project: Option<String>,
    command: String,
    note: String,
}

impl<'a> SessionRow<'a> {
    fn of(session: &'a Session, workspace: &WorkspaceTree) -> Self {
        let or_dash = |text: String| {
            if text.is_empty() {
                "-".to_owned()
            } else {
                text
            }
        };
        Self {
            id: &session.id,
            name: session.name.as_deref(),
            state: session.state,
            hibernated: session.is_hibernated(),
            pid: or_dash(session.pid.map_or_default(|pid| pid.to_string())),
            project: session
                .project(workspace)
                .map(|project| path_of(workspace, &project.id)),
            command: or_dash(session.command.join(" ")),
            note: note(session),
        }
    }
}

/// What a person must know about a session and what has no column: how its
/// command ended, why it hibernated, if a wake failed, if it is pinned, and
/// why its state files are damaged.
fn note(session: &Session) -> String {
    let exit = session.exit.map(|exit| match exit.status {
        ExitStatus::Exited { code } => format!("exit status {code}"),
        ExitStatus::Signaled { signal } => format!("signal {signal}"),
        ExitStatus::Unknown => "exit status not known".to_owned(),
    });
    let hibernation = session
        .hibernation
        .as_ref()
        .filter(|_| session.is_hibernated());
    let reason = hibernation.map(|hibernation| reason_name(hibernation.reason).to_owned());
    let wake_error = hibernation
        .and_then(|hibernation| hibernation.wake_error.as_deref())
        .map(|error| format!("wake failed: {error}"));
    let parts: Vec<&str> = exit
        .as_deref()
        .into_iter()
        .chain(reason.as_deref())
        .chain(wake_error.as_deref())
        .chain(session.pinned.then_some("pinned"))
        .chain(session.damage.as_deref())
        .collect();
    parts.join("; ")
}

/// The name of a reason of a hibernation in the output of the CLI.
pub(super) fn reason_name(reason: HibernationReason) -> &'static str {
    match reason {
        HibernationReason::Count => "count",
        HibernationReason::SessionMemory => "session-memory",
        HibernationReason::TotalMemory => "total-memory",
        HibernationReason::MemoryPressure => "memory-pressure",
        HibernationReason::Request => "request",
        HibernationReason::Unknown => "unknown",
    }
}

/// The state of `session` for a person: a hibernated session is dormant
/// for the protocol, and the CLI names it.
pub(super) fn session_state_name(session: &Session) -> &'static str {
    if session.is_hibernated() {
        "hibernated"
    } else {
        state_name(session.state)
    }
}

fn session_table(
    sessions: &[Session],
    workspace: &WorkspaceTree,
    style: Style,
    width: Option<usize>,
) -> String {
    let rows: Vec<SessionRow<'_>> = sessions
        .iter()
        .map(|session| SessionRow::of(session, workspace))
        .collect();
    // The table has the last column only if a session has a note, the name
    // column only if a session has a name, and the project column only if a
    // session has a Project.
    let has_notes = rows.iter().any(|row| !row.note.is_empty());
    let has_names = rows.iter().any(|row| row.name.is_some());
    let has_projects = rows.iter().any(|row| row.project.is_some());
    let mut header = vec!["ID"];
    if has_names {
        header.push("NAME");
    }
    header.extend(["STATE", "PID"]);
    if has_projects {
        header.push("PROJECT");
    }
    let command = header.len();
    header.extend(["COMMAND", if has_notes { "NOTE" } else { "" }]);
    // A narrow terminal first shortens the command, the note, the Project
    // and the name, then omits the process ID and the Project.
    let pid = header
        .iter()
        .position(|column| *column == "PID")
        .unwrap_or_default();
    let mut shortened = vec![command, command + 1];
    let mut omitted = vec![pid];
    if has_projects {
        shortened.push(pid + 1);
        omitted.push(pid + 1);
    }
    if has_names {
        shortened.push(1);
    }
    let mut table = Table::new(&header).shorten(&shortened).omit(&omitted);
    for row in rows {
        let mut cells = vec![Cell::new(row.id)];
        if has_names {
            cells.push(Cell::new(row.name.unwrap_or("-")));
        }
        cells.extend([
            Cell::coloured(
                if row.hibernated {
                    "hibernated"
                } else {
                    state_name(row.state)
                },
                colour(row.state, style),
            ),
            Cell::new(row.pid),
        ]);
        if has_projects {
            cells.push(Cell::new(row.project.unwrap_or_else(|| "-".to_owned())));
        }
        cells.extend([Cell::new(row.command), Cell::new(row.note)]);
        table.push(cells);
    }
    table.render(width)
}

pub(super) fn column_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values
        .map(|value| value.chars().count())
        .fold(header.len(), usize::max)
}

pub(super) fn state_name(state: SessionState) -> &'static str {
    match state {
        SessionState::Running => "running",
        SessionState::Unreachable => "unreachable",
        SessionState::Dormant => "dormant",
        SessionState::Ended => "ended",
        SessionState::Damaged => "damaged",
        SessionState::Unknown => "unknown",
    }
}

/// The colour of the state of a session.
fn colour(state: SessionState, style: Style) -> Option<&'static str> {
    match (style, state) {
        (Style::Plain, _) | (Style::Colour, SessionState::Ended | SessionState::Unknown) => None,
        (Style::Colour, SessionState::Running) => Some(GREEN),
        (Style::Colour, SessionState::Unreachable) => Some(YELLOW),
        (Style::Colour, SessionState::Dormant) => Some(BLUE),
        (Style::Colour, SessionState::Damaged) => Some(RED),
    }
}

#[cfg(test)]
mod tests {
    use super::super::table::assert_fits;
    use super::*;

    fn session(id: &str, state: SessionState) -> Session {
        Session {
            id: id.to_owned(),
            name: None,
            state,
            pid: None,
            command: Vec::new(),
            working_directory: None,
            size: None,
            started_at: None,
            epoch: None,
            restore_policy: None,
            exit: None,
            damage: None,
            resolved_working_directory: None,
            last_attached_at: None,
            attached_clients: 0,
            hibernation: None,
            pinned: false,
        }
    }

    fn sessions() -> Vec<Session> {
        vec![
            Session {
                pid: Some(7),
                command: vec!["vim".to_owned(), "notes.txt".to_owned()],
                ..session("ab", SessionState::Running)
            },
            session("a-long-identifier", SessionState::Unreachable),
        ]
    }

    #[test]
    fn the_columns_are_aligned_to_the_longest_value() {
        let table = session_table(&sessions(), &WorkspaceTree::default(), Style::Plain, None);

        assert_eq!(
            table,
            "ID                 STATE        PID  COMMAND\n\
             ab                 running      7    vim notes.txt\n\
             a-long-identifier  unreachable  -    -\n"
        );
    }

    #[test]
    fn a_terminal_of_60_columns_shortens_the_command_and_the_note_and_keeps_the_state() {
        use nomux_client::SessionExit;

        let sessions = vec![
            Session {
                pid: Some(4_187_312),
                command: ["claude", "--dangerously-skip-permissions", "--continue"]
                    .map(str::to_owned)
                    .to_vec(),
                ..session("k3xq2abc", SessionState::Running)
            },
            Session {
                command: vec!["make".to_owned()],
                exit: Some(SessionExit {
                    ended_at: 5,
                    status: ExitStatus::Exited { code: 2 },
                }),
                ..session("p2rs6tuv", SessionState::Ended)
            },
            Session {
                damage: Some("line 3 of the session metadata is not valid".to_owned()),
                ..session("u7wq4kda", SessionState::Unreachable)
            },
        ];

        let text = session_table(&sessions, &WorkspaceTree::default(), Style::Plain, Some(60));

        assert_fits(&text, 60);
        assert!(text.contains("4187312  claude"), "{text}");
        assert!(text.contains("unreachable"), "{text}");
        assert!(text.contains('\u{2026}'), "{text}");
    }

    #[test]
    fn the_host_table_fits_a_terminal_of_60_columns() {
        let hosts = [HostEntry {
            name: "build-server".to_owned(),
            ssh: "ssh://deploy@build-server.internal.example.com:2222".to_owned(),
        }];

        let text = host_table(&hosts, Some(60));

        assert_fits(&text, 60);
        assert!(text.contains("build-server  ssh://"), "{text}");
    }

    #[test]
    fn a_narrow_terminal_shortens_the_name_and_leaves_out_the_process_id() {
        let sessions = vec![
            Session {
                name: Some("api-gateway".to_owned()),
                pid: Some(41873),
                command: vec!["cargo".to_owned(), "watch".to_owned(), "-x".to_owned()],
                ..session("k3xq2abc", SessionState::Running)
            },
            session("m4ab5ghi", SessionState::Running),
        ];

        assert_eq!(
            session_table(&sessions, &WorkspaceTree::default(), Style::Plain, Some(40)),
            "ID        NAME        STATE    COMMAND\n\
             k3xq2abc  api-gatew…  running  cargo …\n\
             m4ab5ghi  -           running  -\n"
        );
    }

    #[test]
    fn a_session_without_a_process_shows_why_in_a_note() {
        use nomux_client::SessionExit;

        let sessions = vec![
            Session {
                command: vec!["make".to_owned()],
                exit: Some(SessionExit {
                    ended_at: 5,
                    status: ExitStatus::Exited { code: 2 },
                }),
                ..session("ab", SessionState::Ended)
            },
            Session {
                command: vec!["sh".to_owned()],
                ..session("cd", SessionState::Dormant)
            },
            Session {
                damage: Some("line 3 of the session metadata is not valid".to_owned()),
                ..session("ef", SessionState::Damaged)
            },
        ];

        assert_eq!(
            session_table(&sessions, &WorkspaceTree::default(), Style::Plain, None),
            "ID  STATE    PID  COMMAND  NOTE\n\
             ab  ended    -    make     exit status 2\n\
             cd  dormant  -    sh\n\
             ef  damaged  -    -        line 3 of the session metadata is not valid\n"
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
        let coloured = session_table(&sessions(), &WorkspaceTree::default(), Style::Colour, None);

        assert!(coloured.contains(GREEN));
        let without_colour = [GREEN, YELLOW, RESET]
            .iter()
            .fold(coloured, |text, code| text.replace(code, ""));
        assert_eq!(
            without_colour,
            session_table(&sessions(), &WorkspaceTree::default(), Style::Plain, None)
        );
    }
}
