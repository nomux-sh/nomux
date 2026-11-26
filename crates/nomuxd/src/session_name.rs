//! The name of a session: a text that a user gives to a session and can
//! change. `spec/control.md` of the protocol, "Names", has the rule.

use std::fmt;
use std::str::FromStr;

/// The most characters of a name: the limit of a session identifier.
const MAX_LENGTH: usize = 64;

/// A valid session name: 1 to 64 characters from `A-Z`, `a-z`, `0-9`, `.`,
/// `_` and `-`, and the first one is a letter or a digit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SessionName(String);

impl SessionName {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SessionName {
    type Err = String;

    /// # Errors
    ///
    /// Returns the reason, for a person, why `name` is not a valid name.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let rule = "a name has 1 to 64 characters from A-Z, a-z, 0-9, '.', '_' and '-', and \
                    starts with a letter or a digit";
        let valid_first = name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric());
        let valid_characters = name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_".contains(character));
        if valid_first && valid_characters && name.len() <= MAX_LENGTH {
            Ok(Self(name.to_owned()))
        } else {
            Err(format!("the session name {name:?} is not valid: {rule}"))
        }
    }
}

impl fmt::Display for SessionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_has_the_characters_and_the_length_of_the_rule() {
        for valid in ["api", "a", "9", "api-v2.0_x", &"a".repeat(64)] {
            assert!(valid.parse::<SessionName>().is_ok(), "{valid:?}");
        }
        // The first character cannot look like an option or a hidden file,
        // and the characters of a later target syntax are not permitted.
        for not_valid in [
            "",
            "-api",
            ".api",
            "_api",
            "my api",
            "host:api",
            "group/api",
            "a=b",
            "a@b",
            "über",
            &"a".repeat(65),
        ] {
            assert!(not_valid.parse::<SessionName>().is_err(), "{not_valid:?}");
        }
    }
}
