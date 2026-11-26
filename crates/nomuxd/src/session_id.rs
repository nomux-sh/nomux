//! Makes the identifiers of new sessions.

use std::io;

use nomux_holder_protocol::SessionId;

/// Lower-case letters and digits that a person can read without an error:
/// no `0`, `1`, `8` and `9`.
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
/// 8 characters of 5 random bits each: 2^40 identifiers.
const LENGTH: usize = 8;

/// Makes a random session identifier. The caller makes sure that no session
/// of the host has it.
pub(crate) fn random() -> io::Result<SessionId> {
    random_text(LENGTH)?.parse().map_err(io::Error::other)
}

/// `length` random characters of the alphabet of the session identifiers.
/// The nodes of the workspace tree use it too.
pub(crate) fn random_text(length: usize) -> io::Result<String> {
    let mut bytes = vec![0u8; length];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes
        .iter()
        .map(|byte| char::from(ALPHABET[usize::from(byte & 0x1f)]))
        .collect())
}
