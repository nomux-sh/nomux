//! Answers a journal read request: output bytes of the session go to the
//! daemon in new shared memory.

use nomux_holder_protocol::journal;
use nomux_holder_protocol::wire::{ErrorCode, Reply};

use super::connection::Answer;
use crate::session::Session;
use crate::shared_memory;

/// A reply without bytes has no shared memory: memory of no length cannot
/// be mapped.
pub(super) fn answer(session: &Session, from: u64, max_length: u32) -> Answer {
    let max_length = usize::try_from(max_length).map_or(journal::MAX_RANGE_LENGTH, |length| {
        length.min(journal::MAX_RANGE_LENGTH)
    });
    let Ok((position, bytes)) = session.read_journal(from, max_length) else {
        return Reply::Error(ErrorCode::Failed).into();
    };
    if bytes.is_empty() {
        return Reply::Journal {
            position,
            length: 0,
        }
        .into();
    }
    let write = |memory: &_, bytes: &_| journal::write_range(memory, bytes);
    match shared_memory::with_bytes("journal", &bytes, write) {
        Ok((length, descriptor)) => Answer {
            reply: Reply::Journal { position, length },
            descriptor: Some(descriptor),
        },
        Err(_) => Reply::Error(ErrorCode::Failed).into(),
    }
}
