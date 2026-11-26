//! The memory of the sessions of one `ls --memory` command: the entry of a
//! session, and the totals.

use nomux_client::{MemoryMeasure, Session, SessionMemory};

/// The entry of the session `id`. `None` for a session without a process.
pub(crate) fn memory_of<'a>(memory: &'a [SessionMemory], id: &str) -> Option<&'a SessionMemory> {
    memory.iter().find(|memory| memory.id == id)
}

/// The sums of the memory of the sessions that have a process.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MemoryTotal {
    pub(crate) command_bytes: u64,
    pub(crate) command_processes: u64,
    pub(crate) holder_bytes: u64,
    /// The number of sessions in the sums.
    pub(crate) sessions: u64,
    /// The measure of each session. `None` if the sessions do not have the
    /// same one: the sums then add values of two kinds.
    pub(crate) measure: Option<MemoryMeasure>,
}

/// The memory that the hibernated sessions gave back.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FreedMemory {
    pub(crate) bytes: u64,
    /// The number of hibernated sessions.
    pub(crate) sessions: u64,
}

impl FreedMemory {
    pub(crate) fn of(sessions: &[Session]) -> Self {
        let freed: Vec<u64> = sessions
            .iter()
            .filter(|session| session.is_hibernated())
            .map(|session| freed_bytes(session).unwrap_or(0))
            .collect();
        Self {
            bytes: freed.iter().sum(),
            sessions: freed.len() as u64,
        }
    }
}

/// The memory that the hibernation of `session` gave back. `None` if the
/// session is not hibernated or the daemon did not measure it.
pub(crate) fn freed_bytes(session: &Session) -> Option<u64> {
    session
        .hibernation
        .as_ref()
        .filter(|_| session.is_hibernated())?
        .freed
        .map(|(bytes, _)| bytes)
}

impl MemoryTotal {
    pub(crate) fn of(memory: &[SessionMemory]) -> Self {
        let first = memory.first().map(|memory| memory.measure);
        Self {
            command_bytes: memory.iter().map(|memory| memory.command_bytes).sum(),
            command_processes: memory
                .iter()
                .map(|memory| u64::from(memory.command_processes))
                .sum(),
            holder_bytes: memory.iter().map(|memory| memory.holder_bytes).sum(),
            sessions: memory.len() as u64,
            measure: first.filter(|first| memory.iter().all(|memory| memory.measure == *first)),
        }
    }
}
