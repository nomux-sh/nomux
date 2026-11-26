//! The handler of `Session.Memory` and the form of its result in the
//! control protocol.

use nomux_proto::wire::v1::{
    Error, MemoryMeasure as WireMemoryMeasure, SessionMemory as WireSessionMemory,
    SessionMemoryResponse, response,
};

use super::Context;
use crate::session_memory::{MemoryMeasure, SessionMemory};

pub(super) async fn memory(context: &Context) -> Result<response::Result, Error> {
    let sessions = context.sessions.memory().await?;
    Ok(response::Result::SessionMemory(SessionMemoryResponse {
        sessions: sessions.into_iter().map(WireSessionMemory::from).collect(),
    }))
}

impl From<SessionMemory> for WireSessionMemory {
    fn from(memory: SessionMemory) -> Self {
        let measure = match memory.measure {
            MemoryMeasure::Proportional => WireMemoryMeasure::Proportional,
            MemoryMeasure::Resident => WireMemoryMeasure::Resident,
        };
        Self {
            id: memory.id.to_string(),
            command_bytes: memory.command_bytes,
            command_processes: u32::try_from(memory.command_processes).unwrap_or(u32::MAX),
            holder_bytes: memory.holder_bytes,
            measure: measure.into(),
            limit_bytes: 0,
        }
    }
}
