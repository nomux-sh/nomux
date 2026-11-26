//! The line that a pooled shell gets to change to the working directory of
//! its session. The shell started in the home directory before the session
//! existed, thus the daemon types the change into its terminal.

use std::ffi::OsStr;
use std::path::Path;

/// The shells that read `cd '<directory>'` in the same way. Each of them
/// also keeps a line that starts with a space out of its history when the
/// user asks for it (`ignorespace` of bash, `HIST_IGNORE_SPACE` of zsh, and
/// the default of fish).
const KNOWN_SHELLS: [&str; 7] = ["sh", "bash", "zsh", "dash", "ksh", "mksh", "fish"];

/// `true` if the pool can change the directory of `shell`: the file name of
/// the program is a known shell.
pub(crate) fn is_known_shell(shell: &OsStr) -> bool {
    Path::new(shell)
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| KNOWN_SHELLS.contains(&name))
}

/// The input that changes the directory of a known shell to `directory`.
/// `None` if a known shell can read the path in different ways: the path
/// has a quote, a backslash, a control character or bytes that are not
/// UTF-8.
pub(crate) fn input(directory: &Path) -> Option<Vec<u8>> {
    let text = directory.to_str()?;
    let needs_quoting =
        |character: char| matches!(character, '\'' | '\\') || character.is_control();
    if text.contains(needs_quoting) {
        return None;
    }
    // The space keeps the line out of the history, and the path is
    // absolute, thus it is not an option of `cd`.
    Some(format!(" cd '{text}'\r").into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_a_shell_can_read_in_different_ways_gets_no_input() {
        assert_eq!(
            input(Path::new("/home/user/a b")).as_deref(),
            Some(&b" cd '/home/user/a b'\r"[..])
        );
        for path in ["/tmp/it's", "/tmp/a\\b", "/tmp/a\nb", "/tmp/a\x1bb"] {
            assert_eq!(input(Path::new(path)), None, "{path:?}");
        }
    }

    #[test]
    fn only_the_file_name_of_the_shell_counts() {
        assert!(is_known_shell(OsStr::new("/usr/local/bin/fish")));
        assert!(!is_known_shell(OsStr::new("/usr/bin/nu")));
        assert!(!is_known_shell(OsStr::new("/bin/bash-wrapper")));
    }
}
