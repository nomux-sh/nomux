//! The memory that the processes of the sessions of a host use.

use nomux_proto::wire::v1::{self as wire, SessionMemoryRequest, request, response};

use crate::client::unexpected_result;
use crate::{Client, ClientError};

/// How the daemon measured the memory of the processes of one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMeasure {
    /// The proportional set size (PSS): a page that more than one process
    /// uses is divided between them. A sum counts no page two times.
    Proportional,
    /// The resident set size (RSS): a page that more than one process uses
    /// is in the value of each of them. A sum can be larger than the memory
    /// in use.
    Resident,
    /// A measure from a later protocol version.
    Unknown,
}

/// The memory of the processes of one session, at the time of the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMemory {
    /// The session identifier.
    pub id: String,
    /// The memory of the process tree of the command in bytes: the command
    /// and each process that descends from it, without the holder.
    pub command_bytes: u64,
    /// The number of processes in the process tree of the command.
    pub command_processes: u32,
    /// The memory of the holder of the session in bytes.
    pub holder_bytes: u64,
    /// How the daemon measured the two values.
    pub measure: MemoryMeasure,
}

impl From<wire::SessionMemory> for SessionMemory {
    fn from(memory: wire::SessionMemory) -> Self {
        let measure = match wire::MemoryMeasure::try_from(memory.measure) {
            Ok(wire::MemoryMeasure::Proportional) => MemoryMeasure::Proportional,
            Ok(wire::MemoryMeasure::Resident) => MemoryMeasure::Resident,
            Ok(wire::MemoryMeasure::Unspecified) | Err(_) => MemoryMeasure::Unknown,
        };
        Self {
            id: memory.id,
            command_bytes: memory.command_bytes,
            command_processes: memory.command_processes,
            holder_bytes: memory.holder_bytes,
            measure,
        }
    }
}

impl Client {
    /// Asks the daemon to measure the memory of the processes of each
    /// session that has a holder, in the sequence of the identifiers. A
    /// session without a process is not in the result.
    ///
    /// The daemon measures for this request only. It keeps no value, thus
    /// a client that shows the memory again asks again.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the daemon cannot read the
    /// processes of the host, or if the daemon is older than this request.
    pub async fn session_memory(&mut self) -> Result<Vec<SessionMemory>, ClientError> {
        let request = request::Body::SessionMemory(SessionMemoryRequest {});
        match self.call(request).await? {
            response::Result::SessionMemory(measured) => Ok(measured
                .sessions
                .into_iter()
                .map(SessionMemory::from)
                .collect()),
            _ => Err(unexpected_result()),
        }
    }
}
