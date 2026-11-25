//! OSC 133: the marks with which a shell shows where a prompt, the input of
//! the user and the output of a command start and end.
//!
//! ```text
//! OSC 133 ; A ST               the prompt starts
//! OSC 133 ; B ST               the prompt ends: the input line starts
//! OSC 133 ; C ST               the input line ends: the command runs
//! OSC 133 ; D [; exit code] ST the command ended
//! ```
//!
//! A mark can have options after the letter, for example `A;aid=12`. The
//! parser ignores them.

/// The largest number of bytes of an exit code: `-2147483648`.
const MAX_EXIT_CODE_BYTES: usize = 11;

/// One prompt mark of a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMark {
    /// `A`: the shell starts to write a prompt.
    PromptStart,
    /// `B`: the prompt is complete, and the input line of the user starts.
    InputStart,
    /// `C`: the shell took the input line, and the command starts.
    CommandStart,
    /// `D`: the command ended.
    CommandEnd {
        /// The exit code of the command, if the shell gave it.
        exit_code: Option<i32>,
    },
}

/// Parses the payload of a mark: the bytes after `133;`.
///
/// Returns `None` if the first field is not one of the four letters.
#[must_use]
pub fn parse(payload: &[u8]) -> Option<PromptMark> {
    let mut fields = payload.split(|byte| *byte == b';');
    match fields.next()? {
        b"A" => Some(PromptMark::PromptStart),
        b"B" => Some(PromptMark::InputStart),
        b"C" => Some(PromptMark::CommandStart),
        b"D" => Some(PromptMark::CommandEnd {
            exit_code: fields.next().and_then(parse_exit_code),
        }),
        _ => None,
    }
}

fn parse_exit_code(field: &[u8]) -> Option<i32> {
    if field.len() > MAX_EXIT_CODE_BYTES {
        return None;
    }
    std::str::from_utf8(field).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{PromptMark, parse};

    #[test]
    fn each_letter_gives_its_mark_and_the_options_are_ignored() {
        assert_eq!(parse(b"A"), Some(PromptMark::PromptStart));
        assert_eq!(parse(b"A;aid=12;cl=line"), Some(PromptMark::PromptStart));
        assert_eq!(parse(b"B"), Some(PromptMark::InputStart));
        assert_eq!(parse(b"C;cmdline=ls"), Some(PromptMark::CommandStart));
    }

    #[test]
    fn the_command_end_has_the_exit_code_only_if_the_shell_gave_a_number() {
        let end = |exit_code| Some(PromptMark::CommandEnd { exit_code });

        assert_eq!(parse(b"D"), end(None));
        assert_eq!(parse(b"D;0"), end(Some(0)));
        assert_eq!(parse(b"D;130;aid=12"), end(Some(130)));
        assert_eq!(parse(b"D;-1"), end(Some(-1)));
        assert_eq!(parse(b"D;aid=12"), end(None));
        assert_eq!(parse(b"D;99999999999999999999"), end(None));
        assert_eq!(parse(b"D;\xff"), end(None));
    }

    #[test]
    fn a_first_field_that_is_not_one_of_the_four_letters_is_no_mark() {
        for payload in [&b""[..], b"E", b"AB", b"a", b";A", b" A", b"\xff"] {
            assert_eq!(parse(payload), None, "{payload:?}");
        }
    }
}
