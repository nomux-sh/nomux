//! Answers a redraw request: the redraw sequence of the current screen goes
//! to the daemon in new shared memory.

use nomux_holder_protocol::redraw;
use nomux_holder_protocol::wire::{ErrorCode, Reply};

use super::connection::Answer;
use crate::session::Session;
use crate::shared_memory;

/// The holder has one thread, and it gives each chunk of output to the ring
/// buffer and to the screen state in one step. Thus the position and the
/// sequence are from the same moment.
pub(super) fn answer(session: &mut Session) -> Answer {
    let position = session.redraw_position();
    let sequence = session.redraw_sequence();
    if sequence.len() > redraw::MAX_LENGTH {
        return Reply::Error(ErrorCode::RedrawTooLarge).into();
    }
    let write = |memory: &_, bytes: &_| redraw::write(memory, bytes);
    match shared_memory::with_bytes("redraw", &sequence, write) {
        Ok((length, descriptor)) => Answer {
            reply: Reply::Redraw { position, length },
            descriptor: Some(descriptor),
        },
        Err(_) => Reply::Error(ErrorCode::Failed).into(),
    }
}
