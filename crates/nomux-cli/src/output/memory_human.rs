//! The text form of the sessions of `ls --memory`: one row for each
//! session, and one line with the totals.

use nomux_client::{MemoryMeasure, Session, SessionMemory};

use super::human::{column_width, state_name};
use super::memory_size::memory_size;
use crate::session_memory::{MemoryTotal, memory_of};

const HEADERS: [&str; 7] = [
    "ID",
    "STATE",
    "MEMORY",
    "PROCESSES",
    "HOLDER",
    "MEASURE",
    "COMMAND",
];
const ABSENT: &str = "-";

pub(super) fn session_table(sessions: &[Session], memory: &[SessionMemory]) -> String {
    let rows: Vec<[String; 7]> = sessions
        .iter()
        .map(|session| row(session, memory_of(memory, &session.id)))
        .collect();
    let widths: Vec<usize> = (0..HEADERS.len())
        .map(|column| column_width(HEADERS[column], rows.iter().map(|row| row[column].as_str())))
        .collect();

    let mut table = line(&HEADERS, &widths);
    for row in &rows {
        table.push_str(&line(row, &widths));
    }
    table.push_str(&total_line(&MemoryTotal::of(memory)));
    table
}

fn row(session: &Session, memory: Option<&SessionMemory>) -> [String; 7] {
    let command = match session.command.join(" ") {
        command if command.is_empty() => ABSENT.to_owned(),
        command => command,
    };
    let measured = memory.map_or(
        [
            ABSENT.to_owned(),
            ABSENT.to_owned(),
            ABSENT.to_owned(),
            ABSENT.to_owned(),
        ],
        |memory| {
            [
                memory_size(memory.command_bytes),
                memory.command_processes.to_string(),
                memory_size(memory.holder_bytes),
                measure_name(memory.measure).to_owned(),
            ]
        },
    );
    let [memory, processes, holder, measure] = measured;
    [
        session.id.clone(),
        state_name(session.state).to_owned(),
        memory,
        processes,
        holder,
        measure,
        command,
    ]
}

/// The cells with two spaces between them. The last cell has no padding.
fn line(cells: &[impl AsRef<str>], widths: &[usize]) -> String {
    let padded: Vec<String> = cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{:<width$}", cell.as_ref()))
        .collect();
    format!("{}\n", padded.join("  ").trim_end())
}

fn total_line(total: &MemoryTotal) -> String {
    if total.sessions == 0 {
        return "total: no session has a process\n".to_owned();
    }
    let measure = match total.measure {
        Some(measure) => measure_name(measure),
        None => "pss and rss",
    };
    format!(
        "total: {} in {} of {}, and {} in the holders ({measure})\n",
        memory_size(total.command_bytes),
        counted(total.command_processes, "process", "processes"),
        counted(total.sessions, "session", "sessions"),
        memory_size(total.holder_bytes),
    )
}

fn counted(count: u64, one: &str, more: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        count => format!("{count} {more}"),
    }
}

pub(super) fn measure_name(measure: MemoryMeasure) -> &'static str {
    match measure {
        MemoryMeasure::Proportional => "pss",
        MemoryMeasure::Resident => "rss",
        MemoryMeasure::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use nomux_client::SessionState;

    use super::*;

    const MEBIBYTE: u64 = 1024 * 1024;

    fn session(id: &str, state: SessionState, command: &[&str]) -> Session {
        Session {
            id: id.to_owned(),
            state,
            pid: None,
            command: command.iter().map(|&word| word.to_owned()).collect(),
            working_directory: None,
            size: None,
            started_at: None,
            epoch: None,
            restore_policy: None,
            exit: None,
            damage: None,
        }
    }

    fn memory(id: &str, command_bytes: u64, measure: MemoryMeasure) -> SessionMemory {
        SessionMemory {
            id: id.to_owned(),
            command_bytes,
            command_processes: 3,
            holder_bytes: 2 * MEBIBYTE,
            measure,
        }
    }

    #[test]
    fn a_session_without_a_process_has_no_values_and_is_not_in_the_total() {
        let sessions = [
            session("ab", SessionState::Running, &["claude"]),
            session("cd", SessionState::Dormant, &["vim", "notes.txt"]),
        ];
        let memory = [memory("ab", 512 * MEBIBYTE, MemoryMeasure::Proportional)];

        assert_eq!(
            session_table(&sessions, &memory),
            "ID  STATE    MEMORY     PROCESSES  HOLDER   MEASURE  COMMAND\n\
             ab  running  512.0 MiB  3          2.0 MiB  pss      claude\n\
             cd  dormant  -          -          -        -        vim notes.txt\n\
             total: 512.0 MiB in 3 processes of 1 session, and 2.0 MiB in the holders (pss)\n"
        );
    }

    #[test]
    fn the_total_names_the_two_measures_when_the_sessions_do_not_have_the_same_one() {
        let sessions = [
            session("ab", SessionState::Running, &["a"]),
            session("cd", SessionState::Running, &["b"]),
        ];
        let memory = [
            memory("ab", MEBIBYTE, MemoryMeasure::Proportional),
            memory("cd", MEBIBYTE, MemoryMeasure::Resident),
        ];

        let table = session_table(&sessions, &memory);

        assert_eq!(
            table.lines().last(),
            Some(
                "total: 2.0 MiB in 6 processes of 2 sessions, and 4.0 MiB in the holders (pss and rss)"
            )
        );
        assert_eq!(
            session_table(&sessions[..1], &[]).lines().last(),
            Some("total: no session has a process")
        );
    }
}
