//! Answers a program reports request: the encoded program reports go to the
//! daemon in new shared memory.

use nomux_holder_protocol::program_reports;
use nomux_holder_protocol::wire::{ErrorCode, Reply};

use super::connection::Answer;
use crate::session::Session;
use crate::shared_memory;

/// The limits of the store keep the encoded program reports shorter than
/// `program_reports::MAX_LENGTH`.
pub(super) fn answer(session: &Session) -> Answer {
    let revision = session.program_reports_revision();
    let encoded = session.encoded_program_reports();
    let write = |memory: &_, bytes: &_| program_reports::write(memory, bytes);
    match shared_memory::with_bytes("reports", &encoded, write) {
        Ok((length, descriptor)) => Answer {
            reply: Reply::ProgramReports { revision, length },
            descriptor: Some(descriptor),
        },
        Err(_) => Reply::Error(ErrorCode::Failed).into(),
    }
}
