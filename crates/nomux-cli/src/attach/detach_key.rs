//! The key sequence that detaches the client: `Ctrl-\` and then `d`.
//!
//! `Ctrl-\` is the prefix. In a terminal that is not in raw mode it sends
//! `SIGQUIT`, thus almost no program gives it a function. A program that
//! needs the key gets it when the user presses it two times.

/// `Ctrl-\`: the byte 0x1c.
const PREFIX: u8 = 0x1c;
const DETACH: u8 = b'd';

/// Finds the detach sequence in the bytes that the user types.
#[derive(Debug, Default)]
pub(super) struct DetachKey {
    /// The last byte was the prefix, and the next byte decides what it
    /// means.
    prefix_pressed: bool,
}

impl DetachKey {
    /// Appends to `forward` the bytes of `typed` that go to the session.
    /// Returns `true` if the user typed the detach sequence: the bytes
    /// after the sequence go nowhere.
    ///
    /// After the prefix, `d` detaches, a second prefix sends one prefix to
    /// the session, and each other byte goes to the session together with
    /// the prefix.
    pub(super) fn filter(&mut self, typed: &[u8], forward: &mut Vec<u8>) -> bool {
        for &byte in typed {
            match (std::mem::take(&mut self.prefix_pressed), byte) {
                (false, PREFIX) => self.prefix_pressed = true,
                (false, byte) | (true, byte @ PREFIX) => forward.push(byte),
                (true, DETACH) => return true,
                (true, byte) => forward.extend([PREFIX, byte]),
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gives each chunk to one filter. Returns the bytes for the session and
    /// if the user detached.
    fn typed(chunks: &[&[u8]]) -> (Vec<u8>, bool) {
        let mut key = DetachKey::default();
        let mut forward = Vec::new();
        let detached = chunks.iter().any(|chunk| key.filter(chunk, &mut forward));
        (forward, detached)
    }

    #[test]
    fn the_sequence_detaches_also_when_its_keys_come_in_two_reads() {
        assert_eq!(typed(&[b"ls\r\x1cd"]), (b"ls\r".to_vec(), true));
        assert_eq!(typed(&[b"ls\r\x1c", b"dmore"]), (b"ls\r".to_vec(), true));
    }

    #[test]
    fn the_prefix_reaches_the_session_when_no_detach_follows() {
        // Two times the prefix is one prefix for the program.
        assert_eq!(typed(&[b"a\x1c\x1cd"]), (b"a\x1cd".to_vec(), false));
        // A different key after the prefix: the program gets the two keys.
        assert_eq!(typed(&[b"\x1c", b"x"]), (b"\x1cx".to_vec(), false));
        // A `d` without the prefix is a normal key.
        assert_eq!(typed(&[b"dd"]), (b"dd".to_vec(), false));
    }
}
