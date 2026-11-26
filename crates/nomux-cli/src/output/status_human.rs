//! The text form of the report of a status command.

use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use nomux_client::{
    AgentExplanation, AgentRun, AgentState, AgentStatus, AgentTree, ChildWork, EventBody,
    HostEvent, NeedsYouKind, ReportedState, Session, SourceExplanation, StatusSource,
    WorkspaceTree,
};

use super::human::{BLUE, GREEN, RED, Style, YELLOW, column_width};
use super::table::{Cell, INDENT, Table};
use super::workspace_human;
use crate::status_commands::{StatusReport, WorkspaceStatus};

/// The mark of an empty cell of a table.
const NONE: &str = "-";

pub(super) fn report(report: &StatusReport, style: Style, width: Option<usize>) -> String {
    match report {
        StatusReport::Tree {
            sessions,
            workspace,
            ..
        } if sessions.is_empty()
            && workspace
                .as_ref()
                .is_none_or(|workspace| workspace.tree.is_empty()) =>
        {
            "no sessions\n".to_owned()
        }
        StatusReport::Tree {
            tree,
            sessions,
            with_children,
            workspace,
        } => {
            let workspace = workspace.as_ref().map(|workspace| &workspace.tree);
            let header = if workspace.is_some() {
                "NAME"
            } else {
                "SESSION"
            };
            let rows = rows(tree, sessions, *with_children, workspace);
            status_table(header, &rows, style, width)
        }
        StatusReport::Explained(explanation) => explained(explanation),
        StatusReport::Set { id, state } => format!("session {id}: {}\n", reported(*state)),
        StatusReport::EventsFollowed => String::new(),
    }
}

/// Only what a script needs: the sessions that need the user.
pub(super) fn quiet_report(report: &StatusReport) -> String {
    match report {
        StatusReport::Tree { tree, .. } => tree
            .runs
            .iter()
            .filter(|run| matches!(run.aggregate.state, AgentState::NeedsYou(_)))
            .flat_map(|run| [run.session_id.as_str(), "\n"])
            .collect(),
        _ => String::new(),
    }
}

/// One line for an event of `events --follow`.
pub(super) fn event_line(event: &HostEvent) -> String {
    let seq = event.seq;
    match &event.body {
        EventBody::AgentRunChanged(run) => {
            let status = &run.aggregate;
            let line = format!(
                "{seq} {} {} {}",
                run.session_id,
                state_text(status),
                message_of(status).unwrap_or_default()
            );
            format!("{}\n", line.trim_end())
        }
        EventBody::AgentRunRemoved { session_id } => format!("{seq} {session_id} removed\n"),
        EventBody::ExtensionEvent {
            extension_id,
            name,
            payload,
        } => format!(
            "{seq} extension {extension_id}: {name} ({} bytes)\n",
            payload.len()
        ),
        EventBody::WorkspaceChanged { revision, change } => {
            format!(
                "{seq} {}\n",
                workspace_human::change_line(*revision, change)
            )
        }
        EventBody::Snapshot => format!("{seq} snapshot: the status of each session follows\n"),
        EventBody::Unknown => format!("{seq} an event that this client does not know\n"),
    }
}

/// One row of the status table: a session, or a Child work below it.
struct Row<'a> {
    /// The session identifier, or the name of the Child work with its
    /// indentation.
    name: String,
    command: String,
    status: Option<&'a AgentStatus>,
}

/// The rows of the sessions. With `workspace`, the sessions without a
/// Project come first, then each Group and each Project with the most
/// urgent status below it, and the sessions below their Project.
fn rows<'a>(
    tree: &'a AgentTree,
    sessions: &'a [Session],
    with_children: bool,
    workspace: Option<&'a WorkspaceTree>,
) -> Vec<Row<'a>> {
    let mut rows = Vec::new();
    let Some(workspace) = workspace else {
        for session in sessions {
            if let Some(run) = tree.runs.iter().find(|run| run.session_id == session.id) {
                session_rows(run, sessions, 0, with_children, &mut rows);
            }
        }
        return rows;
    };
    let status = WorkspaceStatus::new(workspace, &tree.runs, sessions);
    for run in status.outside {
        session_rows(run, sessions, 0, with_children, &mut rows);
    }
    for node in status.nodes {
        let mark = if node.node.is_group() { "/" } else { "" };
        rows.push(Row {
            name: format!("{}{}{mark}", INDENT.repeat(node.depth), node.node.name),
            command: String::new(),
            status: node.aggregate,
        });
        for run in node.runs {
            session_rows(run, sessions, node.depth + 1, with_children, &mut rows);
        }
    }
    rows
}

/// The row of the session of `run` at `depth`, and the rows of its Child
/// work below it.
fn session_rows<'a>(
    run: &'a AgentRun,
    sessions: &[Session],
    depth: usize,
    with_children: bool,
    rows: &mut Vec<Row<'a>>,
) {
    let command = sessions
        .iter()
        .find(|session| session.id == run.session_id)
        .map_or_else(String::new, |session| session.command.join(" "));
    rows.push(Row {
        name: format!("{}{}", INDENT.repeat(depth), run.session_id),
        command,
        status: Some(&run.aggregate),
    });
    if with_children {
        child_rows(&run.children, depth + 1, rows);
    }
}

fn child_rows<'a>(children: &'a [ChildWork], depth: usize, rows: &mut Vec<Row<'a>>) {
    for child in children {
        rows.push(Row {
            name: format!("{}{}", INDENT.repeat(depth), child.name),
            command: String::new(),
            status: child.status.as_ref(),
        });
        child_rows(&child.children, depth + 1, rows);
    }
}

/// The table of `rows`. `header` is the header of the column of the names.
fn status_table(header: &str, rows: &[Row<'_>], style: Style, width: Option<usize>) -> String {
    const COMMAND: usize = 1;
    const MESSAGE: usize = 4;
    let now = now_ms();
    // The mark of the state is before the name, in the column of the name.
    let header = format!("  {header}");
    let mut table =
        Table::new(&[&header, "COMMAND", "STATE", "FOR", "MESSAGE"]).shorten(&[COMMAND, MESSAGE]);
    for row in rows {
        let (mark, state, time, message) = match row.status {
            Some(status) => (
                mark(status.state),
                state_text(status),
                duration(now.saturating_sub(status.since_ms)),
                message_of(status).unwrap_or_default().to_owned(),
            ),
            None => (" ", NONE.to_owned(), NONE.to_owned(), String::new()),
        };
        table.push(vec![
            Cell::new(format!("{mark} {}", row.name)),
            Cell::new(&row.command),
            Cell::coloured(state, colour(row.status.map(|status| status.state), style)),
            Cell::new(time),
            Cell::new(message),
        ]);
    }
    table.render(width)
}

fn explained(explanation: &AgentExplanation) -> String {
    let run = &explanation.run;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "session {}: {}",
        run.session_id,
        described(&run.status)
    );
    if run.aggregate != run.status {
        let _ = writeln!(
            text,
            "most urgent with its Child work: {}",
            described(&run.aggregate)
        );
    }
    text.push_str("\nsources, in the sequence in which they decide:\n");
    let width = column_width(
        "",
        explanation
            .sources
            .iter()
            .map(|source| source_name(source.source)),
    );
    for source in &explanation.sources {
        let _ = writeln!(
            text,
            "  {:<width$}  {}",
            source_name(source.source),
            source_line(source)
        );
    }
    if !run.children.is_empty() {
        text.push_str("\nchild work:\n");
        child_lines(&run.children, 1, &mut text);
    }
    if let Some(details) = &run.details {
        text.push_str("\nfrom the adapter:\n");
        let parts = [
            ("model", &details.model),
            ("tool session", &details.provider_session_id),
            ("resume", &details.resume_command),
        ];
        for (name, value) in parts {
            if let Some(value) = value {
                let _ = writeln!(text, "  {name}: {value}");
            }
        }
    }
    text
}

fn child_lines(children: &[ChildWork], depth: usize, text: &mut String) {
    for child in children {
        let status = child
            .status
            .as_ref()
            .map_or_else(|| "no status of its own".to_owned(), described);
        let _ = writeln!(text, "{}{}: {status}", INDENT.repeat(depth), child.name);
        child_lines(&child.children, depth + 1, text);
    }
}

fn source_line(source: &SourceExplanation) -> String {
    match (&source.state, &source.message) {
        (Some(state), Some(message)) => {
            format!("{}: {message} ({})", state_name(*state), source.note)
        }
        (Some(state), None) => format!("{} ({})", state_name(*state), source.note),
        (None, _) => format!("nothing: {}", source.note),
    }
}

/// A status in one line: the state, its time and its source, and the
/// message with its source.
fn described(status: &AgentStatus) -> String {
    let mut text = format!(
        "{} for {}, from {}",
        state_text(status),
        duration(now_ms().saturating_sub(status.since_ms)),
        source_name(status.source)
    );
    if let Some(message) = &status.message {
        let _ = write!(
            text,
            "; message from {}: {}",
            source_name(message.source),
            message.value
        );
    }
    text
}

/// The state with what makes it different: the kind of the wait, a result
/// that the user saw, a guess.
fn state_text(status: &AgentStatus) -> String {
    let mut text = state_name(status.state).to_owned();
    if matches!(status.state, AgentState::Done | AgentState::Error) && status.seen {
        text.push_str(" (seen)");
    }
    if status.source.is_guess() {
        text.push_str(" (guess)");
    }
    text
}

fn message_of(status: &AgentStatus) -> Option<&str> {
    status
        .message
        .as_ref()
        .map(|message| message.value.as_str())
}

pub(super) fn state_name(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "idle",
        AgentState::Working => "working",
        AgentState::Done => "done",
        AgentState::NeedsYou(None) => "needs you",
        AgentState::NeedsYou(Some(NeedsYouKind::Permission)) => "needs you: permission",
        AgentState::NeedsYou(Some(NeedsYouKind::Question)) => "needs you: question",
        AgentState::NeedsYou(Some(NeedsYouKind::Auth)) => "needs you: auth",
        AgentState::Error => "error",
        AgentState::Unknown => "unknown",
    }
}

fn source_name(source: StatusSource) -> &'static str {
    match source {
        StatusSource::None => "no source",
        StatusSource::Adapter => "adapter",
        StatusSource::ProgramStatus => "program status",
        StatusSource::SelfReport => "self-report",
        StatusSource::SessionRestore => "session restore",
        StatusSource::PromptMarks => "prompt marks",
        StatusSource::ForegroundProgram => "foreground program",
        StatusSource::Unknown => "unknown source",
    }
}

fn reported(state: ReportedState) -> &'static str {
    match state {
        ReportedState::Working => "working",
        ReportedState::Done => "done",
        ReportedState::Blocked => "needs you",
        ReportedState::Error => "error",
        ReportedState::Idle => "the report is taken back",
    }
}

/// The mark in front of a state: what needs the user stands out without
/// colour too.
fn mark(state: AgentState) -> &'static str {
    match state {
        AgentState::NeedsYou(_) => "!",
        AgentState::Error => "x",
        AgentState::Done => "+",
        AgentState::Working => "*",
        AgentState::Idle | AgentState::Unknown => " ",
    }
}

/// The colour of a state. A state without a colour is idle or not known.
fn colour(state: Option<AgentState>, style: Style) -> Option<&'static str> {
    match (style, state) {
        (Style::Colour, Some(AgentState::NeedsYou(_))) => Some(YELLOW),
        (Style::Colour, Some(AgentState::Error)) => Some(RED),
        (Style::Colour, Some(AgentState::Done)) => Some(GREEN),
        (Style::Colour, Some(AgentState::Working)) => Some(BLUE),
        _ => None,
    }
}

/// A time as a person reads it: the largest unit only.
fn duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86_400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn now_ms() -> u64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use nomux_client::{AgentRun, SessionState, Sourced};

    use super::super::human::RESET;

    use super::*;

    fn session(id: &str, command: &str) -> Session {
        Session {
            id: id.to_owned(),
            state: SessionState::Running,
            pid: None,
            command: command.split(' ').map(str::to_owned).collect(),
            working_directory: None,
            size: None,
            started_at: None,
            epoch: None,
            restore_policy: None,
            exit: None,
            damage: None,
        }
    }

    fn status(state: AgentState, source: StatusSource, message: Option<&str>) -> AgentStatus {
        AgentStatus {
            state,
            source,
            since_ms: now_ms(),
            seen: false,
            message: message.map(|text| Sourced {
                value: text.to_owned(),
                source,
            }),
            title: None,
            progress: None,
            tool: None,
        }
    }

    fn run(id: &str, status: AgentStatus, children: Vec<ChildWork>) -> AgentRun {
        AgentRun {
            session_id: id.to_owned(),
            aggregate: status.clone(),
            status,
            children,
            details: None,
        }
    }

    fn table(style: Style, with_children: bool) -> String {
        let waiting = status(
            AgentState::NeedsYou(Some(NeedsYouKind::Permission)),
            StatusSource::ProgramStatus,
            Some("run tests?"),
        );
        let child = ChildWork {
            name: "reviewer".to_owned(),
            status: Some(waiting.clone()),
            aggregate: waiting.clone(),
            children: Vec::new(),
        };
        let tree = AgentTree {
            runs: vec![
                run("a1", waiting, vec![child]),
                run(
                    "b22",
                    status(AgentState::Working, StatusSource::ForegroundProgram, None),
                    Vec::new(),
                ),
            ],
            attention: None,
        };
        let sessions = [session("a1", "claude"), session("b22", "make test")];
        status_table(
            "SESSION",
            &rows(&tree, &sessions, with_children, None),
            style,
            None,
        )
    }

    #[test]
    fn each_session_is_one_aligned_row_and_a_guess_says_so() {
        assert_eq!(
            table(Style::Plain, false),
            "  SESSION  COMMAND    STATE                  FOR  MESSAGE\n\
             ! a1       claude     needs you: permission  0s   run tests?\n\
             * b22      make test  working (guess)        0s\n"
        );
    }

    #[test]
    fn the_tree_indents_the_child_work_below_its_session() {
        let table = table(Style::Plain, true);
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[2].starts_with("!   reviewer"), "{table}");
    }

    #[test]
    fn colour_is_only_on_the_state_and_does_not_change_the_alignment() {
        let coloured = table(Style::Colour, false);
        assert!(coloured.contains(&format!("{YELLOW}needs you: permission{RESET}")));
        let without_colour = coloured
            .replace(YELLOW, "")
            .replace(BLUE, "")
            .replace(RESET, "");
        assert_eq!(without_colour, table(Style::Plain, false));
    }
}
