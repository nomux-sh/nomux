//! The prefix keys of an attach: the keys that come before a key of nomux,
//! for example `Ctrl-a` and then `d` to detach. `docs/cli.md`, "Keys", has
//! the rules.

use std::fmt;
use std::str::FromStr;

/// The most prefix keys of one attach.
const MAX_PREFIX_KEYS: usize = 2;
/// The text that turns the prefix off.
const NONE: &str = "none";

/// One prefix key: a control key, as its byte in the legacy encoding of a
/// terminal (0x00 to 0x1f).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixKey(u8);

impl PrefixKey {
    /// `Ctrl-a`.
    pub const CONTROL_A: Self = Self(0x01);
    /// `Ctrl-\`.
    pub const CONTROL_BACKSLASH: Self = Self(0x1c);

    /// The byte of the key in the legacy encoding of a terminal.
    #[must_use]
    pub fn byte(self) -> u8 {
        self.0
    }

    /// The letter of a `Ctrl` and letter key in lower case, for example `a`
    /// for `Ctrl-a`. `None` for a key that is not a letter.
    #[must_use]
    pub fn letter(self) -> Option<char> {
        matches!(self.0, 0x01..=0x1a).then(|| char::from(self.0 + b'a' - 1))
    }
}

impl FromStr for PrefixKey {
    type Err = String;

    /// Reads `C-<x>`: `x` is a letter, `\`, `]`, `^`, `_`, `@` or `Space`.
    ///
    /// # Errors
    ///
    /// Returns the reason, for a person, if the text is not such a key, or
    /// if the key has a function that a program needs: interrupt, end of
    /// input, suspend, flow control, backspace, tab, new line, Enter or
    /// Escape.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let not_valid = || {
            format!(
                "the prefix key {text:?} is not valid: give C-<x>, where x is a letter, \\, ], \
                 ^, _, @ or Space, or give none"
            )
        };
        let key = text.strip_prefix("C-").ok_or_else(not_valid)?;
        let byte = match key {
            "Space" | "@" => 0x00,
            "\\" => 0x1c,
            "]" => 0x1d,
            "^" => 0x1e,
            "_" => 0x1f,
            letter if letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic() => {
                letter.as_bytes()[0].to_ascii_lowercase() - b'a' + 1
            }
            _ => return Err(not_valid()),
        };
        // C-c, C-d, C-h, C-i, C-j, C-m, C-q, C-s and C-z.
        if [0x03, 0x04, 0x08, 0x09, 0x0a, 0x0d, 0x11, 0x13, 0x1a].contains(&byte) {
            return Err(format!(
                "the prefix key {text} cannot be used: programs need it (interrupt, end of \
                 input, backspace, tab, new line, Enter, flow control or suspend)"
            ));
        }
        Ok(Self(byte))
    }
}

impl fmt::Display for PrefixKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.0, self.letter()) {
            (_, Some(letter)) => write!(formatter, "C-{letter}"),
            (0x00, None) => formatter.write_str("C-Space"),
            (0x1c, None) => formatter.write_str("C-\\"),
            (0x1d, None) => formatter.write_str("C-]"),
            (0x1e, None) => formatter.write_str("C-^"),
            _ => formatter.write_str("C-_"),
        }
    }
}

/// The prefix keys of an attach: none, one or two keys. Without a key, no
/// key detaches and each key goes to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixKeys(Vec<PrefixKey>);

impl Default for PrefixKeys {
    /// `Ctrl-a`, the key of screen, and `Ctrl-\`, the key of nomux 0.4.
    fn default() -> Self {
        Self(vec![PrefixKey::CONTROL_A, PrefixKey::CONTROL_BACKSLASH])
    }
}

impl PrefixKeys {
    /// The keys in the sequence of the setting.
    #[must_use]
    pub fn keys(&self) -> &[PrefixKey] {
        &self.0
    }

    /// The keys of `texts`: each one a key, or one `none`.
    ///
    /// # Errors
    ///
    /// Returns the reason, for a person, if a key is not valid, if there
    /// are more than two keys, if a key is two times in the list, or if
    /// `none` is not alone.
    pub fn from_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Self, String> {
        let texts: Vec<&str> = texts.into_iter().map(str::trim).collect();
        if texts == [NONE] {
            return Ok(Self(Vec::new()));
        }
        let keys = texts
            .iter()
            .map(|text| text.parse())
            .collect::<Result<Vec<PrefixKey>, String>>()?;
        if keys.len() > MAX_PREFIX_KEYS {
            return Err(format!(
                "an attach has {MAX_PREFIX_KEYS} prefix keys at most"
            ));
        }
        if keys.len() == MAX_PREFIX_KEYS && keys[0] == keys[1] {
            return Err(format!(
                "the prefix key {} is two times in the list",
                keys[0]
            ));
        }
        Ok(Self(keys))
    }
}

impl FromStr for PrefixKeys {
    type Err = String;

    /// Reads the keys with a comma between them, for example `C-a,C-\`, or
    /// `none`: the form of `NOMUX_PREFIX`.
    ///
    /// # Errors
    ///
    /// Returns the reason, for a person, as [`PrefixKeys::from_texts`].
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::from_texts(text.split(','))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_control_key_that_programs_do_not_need() {
        let byte = |text: &str| text.parse::<PrefixKey>().map(PrefixKey::byte);
        assert_eq!(byte("C-a"), Ok(0x01));
        assert_eq!(byte("C-B"), Ok(0x02));
        assert_eq!(byte("C-\\"), Ok(0x1c));
        assert_eq!(byte("C-]"), Ok(0x1d));
        assert_eq!(byte("C-Space"), Ok(0x00));
        for refused in [
            "C-c", "C-d", "C-z", "C-s", "C-q", "C-h", "C-i", "C-j", "C-m",
        ] {
            assert!(byte(refused).is_err(), "{refused}");
        }
        // C-[ is Escape. A key without C- is not a control key.
        for not_valid in ["C-[", "a", "C-", "C-ab", "M-a", "Ctrl-a"] {
            assert!(byte(not_valid).is_err(), "{not_valid}");
        }
        for key in ["C-a", "C-\\", "C-Space", "C-^"] {
            assert_eq!(key.parse::<PrefixKey>().unwrap().to_string(), key);
        }
    }

    #[test]
    fn a_list_has_two_different_keys_at_most_or_none() {
        assert_eq!("C-a,C-\\".parse(), Ok(PrefixKeys::default()));
        assert_eq!("none".parse(), Ok(PrefixKeys(Vec::new())));
        assert_eq!(
            "C-b".parse::<PrefixKeys>().map(|keys| keys.keys().to_vec()),
            Ok(vec![PrefixKey(0x02)])
        );
        assert!("C-a,C-a".parse::<PrefixKeys>().is_err());
        assert!("C-a,C-b,C-]".parse::<PrefixKeys>().is_err());
        assert!("C-a,none".parse::<PrefixKeys>().is_err());
    }
}
