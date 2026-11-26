//! The memory that the processes of each session use, measured when a
//! client asks. The daemon keeps no value and has no task that measures.
//!
//! The command of a session is the only child of its holder. Thus the
//! process tree of the command is each process that descends from the
//! holder, and the daemon needs only the identity of the holder, which the
//! `pid` file of the holder has. `docs/memory.md` has the rules.

pub(crate) mod process_list;
mod process_memory;
mod process_table;

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::ProcessIdentity;

use self::process_memory::ProcessMemory;
use self::process_table::{Descendant, ProcessTable};

/// How the memory of the processes of one session was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MemoryMeasure {
    /// The proportional set size: a sum counts no shared page two times.
    Proportional,
    /// The resident set size: a sum counts a shared page one time for each
    /// process that uses it.
    Resident,
}

/// The memory of the processes of one session, in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMemory {
    pub(crate) id: SessionId,
    /// The process tree of the command, without the holder.
    pub(crate) command_bytes: u64,
    pub(crate) command_processes: usize,
    pub(crate) holder_bytes: u64,
    pub(crate) measure: MemoryMeasure,
}

/// Measures the sessions whose holders have the `pid` files `holders`. A
/// session whose holder does not run has no entry in the result.
///
/// The function reads the process table of the host: call it on a thread
/// that can wait.
///
/// # Errors
///
/// Returns an error if the process table cannot be read.
pub(crate) fn measure(holders: Vec<(SessionId, PathBuf)>) -> io::Result<Vec<SessionMemory>> {
    let table = ProcessTable::read()?;
    Ok(holders
        .into_iter()
        .filter_map(|(id, pid_file)| {
            let holder = fs::read_to_string(pid_file).ok()?.parse().ok()?;
            measure_session(id, holder, &table)
        })
        .collect())
}

fn measure_session(
    id: SessionId,
    holder: ProcessIdentity,
    table: &ProcessTable,
) -> Option<SessionMemory> {
    if !table.has(holder) {
        return None;
    }
    let holder_memory = process_memory::read(holder.pid)?;
    let measured: Vec<(Descendant, ProcessMemory)> = table
        .descendants(holder.pid)
        .into_iter()
        .filter_map(|process| Some((process, process_memory::read(process.identity.pid)?)))
        .collect();
    // The table and the memory values are from different moments. A process
    // that has the same identity after the two reads was the same process
    // at each of them.
    if !still_runs(holder) {
        return None;
    }
    let command = confirmed(holder.pid, measured, still_runs);
    let (measure, holder_bytes, command_bytes) = sum(holder_memory, &command);
    Some(SessionMemory {
        id,
        command_bytes,
        command_processes: command.len(),
        holder_bytes,
        measure,
    })
}

fn still_runs(identity: ProcessIdentity) -> bool {
    matches!(ProcessIdentity::of(identity.pid), Ok(Some(now)) if now == identity)
}

/// The memory of the processes of `measured` that are sure to descend from
/// `root`: a process counts only if it and each process between it and
/// `root` did not change. `measured` has each parent before its children.
///
/// A process ID that the operating system gave to a new process can thus
/// not bring a process of a different program into the sum. The cost is a
/// value that is too small for a moment, while a part of the tree ends.
fn confirmed(
    root: u32,
    measured: Vec<(Descendant, ProcessMemory)>,
    still_runs: impl Fn(ProcessIdentity) -> bool,
) -> Vec<ProcessMemory> {
    let mut confirmed_pids = HashSet::from([root]);
    let mut memories = Vec::new();
    for (process, memory) in measured {
        if confirmed_pids.contains(&process.parent) && still_runs(process.identity) {
            confirmed_pids.insert(process.identity.pid);
            memories.push(memory);
        }
    }
    memories
}

/// The measure of one session, the bytes of its holder and the bytes of its
/// command. The proportional values apply only if each process has one: a
/// sum of the two measures has no meaning.
fn sum(holder: ProcessMemory, command: &[ProcessMemory]) -> (MemoryMeasure, u64, u64) {
    let proportional = holder.proportional.zip(
        command
            .iter()
            .map(|memory| memory.proportional)
            .sum::<Option<u64>>(),
    );
    match proportional {
        Some((holder_bytes, command_bytes)) => {
            (MemoryMeasure::Proportional, holder_bytes, command_bytes)
        }
        None => (
            MemoryMeasure::Resident,
            holder.resident,
            command.iter().map(|memory| memory.resident).sum(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent: u32) -> (Descendant, ProcessMemory) {
        let identity = ProcessIdentity {
            pid,
            start_time: u64::from(pid),
        };
        let memory = ProcessMemory {
            resident: u64::from(pid),
            proportional: None,
        };
        (Descendant { identity, parent }, memory)
    }

    #[test]
    fn a_process_that_changed_takes_its_descendants_out_of_the_sum() {
        // 1 is the holder. 20 changed after the table was read, thus its
        // child 21 can be the child of a process of a different program.
        let measured = vec![
            process(10, 1),
            process(20, 1),
            process(11, 10),
            process(21, 20),
        ];

        let counted = confirmed(1, measured, |identity| identity.pid != 20);

        let pids: Vec<u64> = counted.iter().map(|memory| memory.resident).collect();
        assert_eq!(pids, [10, 11]);
    }

    #[test]
    fn one_process_without_a_proportional_value_makes_each_value_resident() {
        let with = |resident, proportional| ProcessMemory {
            resident,
            proportional,
        };

        assert_eq!(
            sum(with(8, Some(2)), &[with(100, Some(60)), with(50, Some(30))]),
            (MemoryMeasure::Proportional, 2, 90)
        );
        assert_eq!(
            sum(with(8, Some(2)), &[with(100, Some(60)), with(50, None)]),
            (MemoryMeasure::Resident, 8, 150)
        );
        assert_eq!(
            sum(with(8, None), &[with(100, Some(60))]),
            (MemoryMeasure::Resident, 8, 100)
        );
        // A holder whose command has no process yet.
        assert_eq!(
            sum(with(8, Some(2)), &[]),
            (MemoryMeasure::Proportional, 2, 0)
        );
    }
}
