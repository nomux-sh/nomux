//! The JSON form of the sessions of `ls --memory`. `docs/cli.md` has the
//! schema.

use nomux_client::{Session, SessionMemory};
use serde::Serialize;

use super::json::SessionDocument;
use super::memory_human::measure_name;
use crate::session_memory::{FreedMemory, MemoryTotal, memory_of};

/// The fields of the document of `ls --memory`.
#[derive(Serialize)]
pub(super) struct SessionsMemoryBody<'a> {
    sessions: Vec<MeasuredSessionDocument<'a>>,
    memory_total: TotalDocument,
}

/// The object of a session with one more field.
#[derive(Serialize)]
struct MeasuredSessionDocument<'a> {
    #[serde(flatten)]
    session: SessionDocument<'a>,
    /// `null` for a session without a process.
    memory: Option<MemoryDocument>,
}

#[derive(Serialize)]
struct MemoryDocument {
    command_bytes: u64,
    command_processes: u32,
    holder_bytes: u64,
    measure: &'static str,
    /// `null` if the operating system applies no limit.
    limit_bytes: Option<u64>,
}

#[derive(Serialize)]
struct TotalDocument {
    command_bytes: u64,
    command_processes: u64,
    holder_bytes: u64,
    sessions: u64,
    /// `null` if the sessions do not have the same measure, or if no session
    /// has a process.
    measure: Option<&'static str>,
    /// The memory that the hibernated sessions gave back, and their number.
    freed_bytes: u64,
    hibernated_sessions: u64,
}

impl<'a> SessionsMemoryBody<'a> {
    pub(super) fn new(sessions: &'a [Session], memory: &[SessionMemory]) -> Self {
        let total = MemoryTotal::of(memory);
        let freed = FreedMemory::of(sessions);
        Self {
            sessions: sessions
                .iter()
                .map(|session| MeasuredSessionDocument {
                    session: session.into(),
                    memory: memory_of(memory, &session.id).map(|memory| MemoryDocument {
                        command_bytes: memory.command_bytes,
                        command_processes: memory.command_processes,
                        holder_bytes: memory.holder_bytes,
                        measure: measure_name(memory.measure),
                        limit_bytes: memory.limit_bytes,
                    }),
                })
                .collect(),
            memory_total: TotalDocument {
                command_bytes: total.command_bytes,
                command_processes: total.command_processes,
                holder_bytes: total.holder_bytes,
                sessions: total.sessions,
                measure: total.measure.map(measure_name),
                freed_bytes: freed.bytes,
                hibernated_sessions: freed.sessions,
            },
        }
    }
}
