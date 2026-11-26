//! The names of the keys of `send -k`: the names of tmux. A cursor key has
//! the sequence of the normal cursor mode.

/// The bytes of the key `name`.
///
/// # Errors
///
/// Returns the reason, for a person, if no key has the name.
pub(super) fn key_bytes(name: &str) -> Result<Vec<u8>, String> {
    let fixed: Option<&[u8]> = match name {
        "Enter" => Some(b"\r"),
        "Escape" => Some(b"\x1b"),
        "Tab" => Some(b"\t"),
        "S-Tab" => Some(b"\x1b[Z"),
        "Space" => Some(b" "),
        "BSpace" => Some(b"\x7f"),
        "Up" => Some(b"\x1b[A"),
        "Down" => Some(b"\x1b[B"),
        "Right" => Some(b"\x1b[C"),
        "Left" => Some(b"\x1b[D"),
        "Home" => Some(b"\x1b[H"),
        "End" => Some(b"\x1b[F"),
        "PgUp" => Some(b"\x1b[5~"),
        "PgDn" => Some(b"\x1b[6~"),
        "F1" => Some(b"\x1bOP"),
        "F2" => Some(b"\x1bOQ"),
        "F3" => Some(b"\x1bOR"),
        "F4" => Some(b"\x1bOS"),
        "F5" => Some(b"\x1b[15~"),
        "F6" => Some(b"\x1b[17~"),
        "F7" => Some(b"\x1b[18~"),
        "F8" => Some(b"\x1b[19~"),
        "F9" => Some(b"\x1b[20~"),
        "F10" => Some(b"\x1b[21~"),
        "F11" => Some(b"\x1b[23~"),
        "F12" => Some(b"\x1b[24~"),
        _ => None,
    };
    if let Some(bytes) = fixed {
        return Ok(bytes.to_vec());
    }
    let unknown = || {
        format!(
            "{name:?} is not a key name: give Enter, Escape, Tab, S-Tab, Space, BSpace, Up, Down, \
             Left, Right, Home, End, PgUp, PgDn, F1 to F12, C-<letter> or M-<character>"
        )
    };
    if let Some(letter) = name.strip_prefix("C-") {
        return match letter.as_bytes() {
            [letter] if letter.is_ascii_alphabetic() => {
                Ok(vec![letter.to_ascii_lowercase() - b'a' + 1])
            }
            _ => Err(unknown()),
        };
    }
    if let Some(character) = name.strip_prefix("M-")
        && character.chars().count() == 1
    {
        return Ok([b"\x1b", character.as_bytes()].concat());
    }
    Err(unknown())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_name_gives_the_bytes_of_the_key() {
        assert_eq!(key_bytes("C-c"), Ok(vec![0x03]));
        assert_eq!(key_bytes("C-A"), Ok(vec![0x01]));
        assert_eq!(key_bytes("Enter"), Ok(b"\r".to_vec()));
        assert_eq!(key_bytes("Up"), Ok(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes("F12"), Ok(b"\x1b[24~".to_vec()));
        assert_eq!(key_bytes("M-x"), Ok(b"\x1bx".to_vec()));
        for unknown in ["enter", "C-", "C-1", "M-", "M-ab", "F13", "x"] {
            assert!(key_bytes(unknown).is_err(), "{unknown}");
        }
    }
}
