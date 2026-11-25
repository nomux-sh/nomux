//! Answers a redraw request: the redraw sequence of the current screen goes
//! to the daemon in new shared memory.

use std::os::fd::OwnedFd;

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
    match share(&session.redraw_sequence()) {
        Ok((length, descriptor)) => Answer {
            reply: Reply::Redraw { position, length },
            descriptor: Some(descriptor),
        },
        Err(code) => Reply::Error(code).into(),
    }
}

/// Copies `sequence` into new shared memory. Returns its length and the
/// read-only descriptor of the memory.
fn share(sequence: &[u8]) -> Result<(u32, OwnedFd), ErrorCode> {
    let length = u32::try_from(sequence.len())
        .ok()
        .filter(|_| sequence.len() <= redraw::MAX_LENGTH)
        .ok_or(ErrorCode::RedrawTooLarge)?;
    let memory = shared_memory::create("redraw", sequence.len()).map_err(|_| ErrorCode::Failed)?;
    redraw::write(&memory.writable, sequence).map_err(|_| ErrorCode::Failed)?;
    Ok((length, memory.read_only))
}
