//! The standard input of a hook: the data of the event, with a limit of its
//! size and of the time to read it.

use std::io::{self, IsTerminal, Read};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use nomux_core::hook_event::MAX_HOOK_PAYLOAD;

/// The data of a hook event.
pub(super) struct HookInput {
    pub(super) payload: Vec<u8>,
    /// `true` if the input was too large, or did not end before the
    /// deadline: the event goes without its data.
    pub(super) omitted: bool,
}

/// Reads the standard input until its end, [`MAX_HOOK_PAYLOAD`] bytes at
/// most, until `deadline`. A terminal is no input.
///
/// A thread reads, because a read cannot stop at a deadline. The process
/// ends after the hook, and the thread ends with it.
pub(super) fn read(deadline: Instant) -> HookInput {
    if io::stdin().is_terminal() {
        return HookInput {
            payload: Vec::new(),
            omitted: false,
        };
    }
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut payload = Vec::new();
        let limit = u64::try_from(MAX_HOOK_PAYLOAD + 1).unwrap_or(u64::MAX);
        let read = io::stdin().lock().take(limit).read_to_end(&mut payload);
        // The receiver is gone after the deadline.
        let _ = sender.send(read.map(|_| payload));
    });
    let wait = deadline.saturating_duration_since(Instant::now());
    match receiver.recv_timeout(wait) {
        Ok(Ok(payload)) if payload.len() <= MAX_HOOK_PAYLOAD => HookInput {
            payload,
            omitted: false,
        },
        Ok(Err(_)) => HookInput {
            payload: Vec::new(),
            omitted: false,
        },
        Ok(Ok(_)) | Err(_) => HookInput {
            payload: Vec::new(),
            omitted: true,
        },
    }
}
