//! One host of the configuration file.

/// A remote host: `[hosts.<name>]` with the key `ssh`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEntry {
    /// The name that the `--host` option takes.
    pub name: String,
    /// The SSH target: `[user@]host`, `ssh://[user@]host[:port]`, or a
    /// `Host` name of `~/.ssh/config`.
    pub ssh: String,
}

/// A host name has only ASCII letters, digits, `.`, `_` and `-`, and it
/// starts with a letter or a digit. Thus a name is never an option and
/// needs no quotes in a shell.
#[must_use]
pub fn is_valid_host_name(name: &str) -> bool {
    let is_word = |character: char| character.is_ascii_alphanumeric();
    name.starts_with(is_word)
        && name
            .chars()
            .all(|character| is_word(character) || matches!(character, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_name_is_safe_on_a_command_line() {
        assert!(is_valid_host_name("vps"));
        assert!(is_valid_host_name("vps-2.example_com"));
        assert!(is_valid_host_name("10.0.0.7"));

        assert!(!is_valid_host_name(""));
        assert!(!is_valid_host_name("-vps"));
        assert!(!is_valid_host_name("a b"));
        assert!(!is_valid_host_name("user@vps"));
        assert!(!is_valid_host_name("[::1]"));
    }
}
