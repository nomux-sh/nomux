//! The values of the session metadata as text. A value is one line of
//! UTF-8, and it can be each byte sequence: an argument of a command and a
//! path are not always text.
//!
//! | In the file | Byte |
//! |---|---|
//! | `\\` | `\` |
//! | `\n` | Line feed |
//! | `\r` | Carriage return |
//! | `\xHH` | The byte with the hexadecimal value `HH` |
//!
//! The writer uses `\xHH` for the other ASCII control characters and for
//! each byte that is not a part of valid UTF-8.

use std::fmt::Write;

pub(crate) fn escape(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        for character in chunk.valid().chars() {
            match character {
                '\\' => text.push_str(r"\\"),
                '\n' => text.push_str(r"\n"),
                '\r' => text.push_str(r"\r"),
                character if character.is_ascii_control() => push_byte(&mut text, character as u8),
                character => text.push(character),
            }
        }
        for byte in chunk.invalid() {
            push_byte(&mut text, *byte);
        }
    }
    text
}

fn push_byte(text: &mut String, byte: u8) {
    // A write to a `String` does not fail.
    let _ = write!(text, r"\x{byte:02x}");
}

/// Returns `None` if `text` has a `\` that starts no known sequence.
pub(crate) fn unescape(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        rest = after;
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let (&escape, after) = rest.split_first()?;
        rest = after;
        match escape {
            b'\\' => bytes.push(b'\\'),
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b'x' => {
                let (digits, after) = rest.split_first_chunk::<2>()?;
                rest = after;
                bytes.push(hex_digit(digits[0])? << 4 | hex_digit(digits[1])?);
            }
            _ => return None,
        }
    }
    Some(bytes)
}

fn hex_digit(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_byte_sequence_is_one_line_of_text_and_comes_back() {
        let all_bytes: Vec<u8> = (0..=u8::MAX).collect();
        for bytes in [
            all_bytes.as_slice(),
            b"plain text",
            "\u{e7}ay \u{1f375}".as_bytes(),
            b"line\nfeed\\n \\x41",
            b" space at the two ends ",
            b"\xff\xfe cut \xe2\x82",
        ] {
            let text = escape(bytes);
            assert!(!text.contains(['\n', '\r']), "{text:?}");
            assert_eq!(unescape(&text).as_deref(), Some(bytes), "{text:?}");
        }
    }

    #[test]
    fn a_backslash_without_a_known_sequence_is_refused() {
        for text in [r"\", r"\q", r"\x4", r"\xzz", r"end\"] {
            assert_eq!(unescape(text), None, "{text:?}");
        }
    }
}
