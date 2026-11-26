//! The text form of the sessions of `ls --memory`: one row for each
//! session, and one line with the totals. A hibernated session shows the
//! memory that its hibernation gave back.

use nomux_client::{MemoryMeasure, Session, SessionMemory};

use super::human::session_state_name;
use super::memory_size::memory_size;
use super::table::{Cell, Table};
use super::wrapped_line::wrapped;
use crate::session_memory::{FreedMemory, MemoryTotal, freed_bytes, memory_of};

const HEADERS: [&str; 9] = [
    "ID",
    "STATE",
    "MEMORY",
    "PROCESSES",
    "HOLDER",
    "MEASURE",
    "LIMIT",
    "FREED",
    "COMMAND",
];
const ABSENT: &str = "-";

pub(super) fn session_table(
    sessions: &[Session],
    memory: &[SessionMemory],
    width: Option<usize>,
) -> String {
    const PROCESSES: usize = 3;
    const HOLDER: usize = 4;
    const MEASURE: usize = 5;
    const LIMIT: usize = 6;
    const FREED: usize = 7;
    const COMMAND: usize = 8;
    // The total line names the measure, the last line has the memory that
    // the hibernations gave back, and the memory of the command matters more
    // than its number of processes and its holder.
    let mut table = Table::new(&HEADERS)
        .shorten(&[COMMAND])
        .omit(&[MEASURE, LIMIT, FREED, PROCESSES, HOLDER]);
    for session in sessions {
        let row = row(session, memory_of(memory, &session.id));
        table.push(row.into_iter().map(Cell::new).collect());
    }
    let mut text = table.render(width);
    text.push_str(&wrapped(&total_line(&MemoryTotal::of(memory)), width));
    if let Some(line) = freed_line(&FreedMemory::of(sessions)) {
        text.push_str(&wrapped(&line, width));
    }
    text
}

fn row(session: &Session, memory: Option<&SessionMemory>) -> [String; 9] {
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
            ABSENT.to_owned(),
        ],
        |memory| {
            [
                memory_size(memory.command_bytes),
                memory.command_processes.to_string(),
                memory_size(memory.holder_bytes),
                measure_name(memory.measure).to_owned(),
                memory
                    .limit_bytes
                    .map_or_else(|| ABSENT.to_owned(), memory_size),
            ]
        },
    );
    let [memory, processes, holder, measure, limit] = measured;
    let freed = freed_bytes(session).map_or_else(|| ABSENT.to_owned(), memory_size);
    [
        session.id.clone(),
        session_state_name(session).to_owned(),
        memory,
        processes,
        holder,
        measure,
        limit,
        freed,
        command,
    ]
}

/// The memory that the hibernated sessions gave back. `None` without a
/// hibernated session.
fn freed_line(freed: &FreedMemory) -> Option<String> {
    (freed.sessions > 0).then(|| {
        format!(
            "hibernated: {} given back by {}",
            memory_size(freed.bytes),
            counted(freed.sessions, "session", "sessions")
        )
    })
}

fn total_line(total: &MemoryTotal) -> String {
    if total.sessions == 0 {
        return "total: no session has a process".to_owned();
    }
    let measure = match total.measure {
        Some(measure) => measure_name(measure),
        None => "pss and rss",
    };
    format!(
        "total: {} in {} of {}, and {} in the holders ({measure})",
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

    use super::super::table::assert_fits;
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
            name: None,
            last_attached_at: None,
            attached_clients: 0,
            resolved_working_directory: None,
            hibernation: None,
            pinned: false,
            sandbox: None,
        }
    }

    fn memory(id: &str, command_bytes: u64, measure: MemoryMeasure) -> SessionMemory {
        SessionMemory {
            id: id.to_owned(),
            command_bytes,
            command_processes: 3,
            holder_bytes: 2 * MEBIBYTE,
            measure,
            limit_bytes: None,
        }
    }

    #[test]
    fn a_session_without_a_process_has_no_values_and_a_hibernated_one_has_the_memory_it_gave_back()
    {
        use nomux_client::{Hibernation, HibernationReason};

        let hibernated = Session {
            hibernation: Some(Hibernation {
                hibernated_at: 5,
                reason: HibernationReason::Count,
                freed: Some((300 * MEBIBYTE, MemoryMeasure::Proportional)),
                resume_command: "claude --resume 1".to_owned(),
                woken_at: None,
                wake_error: None,
            }),
            ..session("ef", SessionState::Dormant, &["claude"])
        };
        let sessions = [
            session("ab", SessionState::Running, &["claude"]),
            session("cd", SessionState::Dormant, &["vim", "notes.txt"]),
            hibernated,
        ];
        let memory = [SessionMemory {
            limit_bytes: Some(4096 * MEBIBYTE),
            ..memory("ab", 512 * MEBIBYTE, MemoryMeasure::Proportional)
        }];

        assert_eq!(
            session_table(&sessions, &memory, None),
            "ID  STATE       MEMORY     PROCESSES  HOLDER   MEASURE  LIMIT    FREED      COMMAND\n\
             ab  running     512.0 MiB  3          2.0 MiB  pss      4.0 GiB  -          claude\n\
             cd  dormant     -          -          -        -        -        -          vim notes.txt\n\
             ef  hibernated  -          -          -        -        -        300.0 MiB  claude\n\
             total: 512.0 MiB in 3 processes of 1 session, and 2.0 MiB in the holders (pss)\n\
             hibernated: 300.0 MiB given back by 1 session\n"
        );
    }

    #[test]
    fn a_terminal_of_60_columns_leaves_out_the_measure_and_breaks_the_total_line() {
        let sessions = [
            session("2anrctpr", SessionState::Running, &["/bin/zsh"]),
            session(
                "emh2yo3q",
                SessionState::Dormant,
                &["sh", "-c", "sleep 600"],
            ),
        ];
        let memory = [memory("2anrctpr", 3 * MEBIBYTE, MemoryMeasure::Resident)];

        let text = session_table(&sessions, &memory, Some(60));

        assert_eq!(
            text,
            "ID        STATE    MEMORY   PROCESSES  HOLDER   COMMAND\n\
             2anrctpr  running  3.0 MiB  3          2.0 MiB  /bin/zsh\n\
             emh2yo3q  dormant  -        -          -        sh -c sleep\u{2026}\n\
             total: 3.0 MiB in 3 processes of 1 session, and 2.0 MiB in\n\
             the holders (rss)\n"
        );
        assert_fits(&text, 60);
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

        let table = session_table(&sessions, &memory, None);

        assert_eq!(
            table.lines().last(),
            Some(
                "total: 2.0 MiB in 6 processes of 2 sessions, and 4.0 MiB in the holders (pss and rss)"
            )
        );
        assert_eq!(
            session_table(&sessions[..1], &[], None).lines().last(),
            Some("total: no session has a process")
        );
    }
}
