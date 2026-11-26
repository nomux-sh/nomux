//! The words of a resume command. A resume command is text from an
//! extension, thus it never goes to a shell: the daemon starts its first
//! word as the program, with the other words as the arguments.
//!
//! The text has the quoting of a POSIX shell and nothing else: single
//! quotes, double quotes and a backslash. A character that a shell expands
//! or that is an operator of a shell is refused outside single quotes, thus
//! a command means the same here and in a shell. `spec/control.md` of the
//! `proto` repository has the rule.

use std::fmt;

/// Why a resume command does not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WordsError {
    /// The command has no word.
    Empty,
    /// A shell gives the character a meaning that is not text.
    ShellCharacter(char),
    /// A quote or a backslash has no end.
    Unterminated,
}

impl fmt::Display for WordsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("the resume command is empty"),
            Self::ShellCharacter('\n') => {
                formatter.write_str("the resume command has a line break")
            }
            Self::ShellCharacter(character) => write!(
                formatter,
                "the resume command has the shell character {character:?}: nomux does not give \
                 a resume command to a shell"
            ),
            Self::Unterminated => {
                formatter.write_str("the resume command has a quote or a backslash without its end")
            }
        }
    }
}

/// Characters that a shell reads as an operator, an expansion or a comment
/// outside quotes.
const SHELL_CHARACTERS: &[char] = &[
    ';', '|', '&', '<', '>', '(', ')', '$', '`', '*', '?', '[', '#', '~', '\n',
];

/// Characters that a shell expands in double quotes.
const EXPANDED_IN_DOUBLE_QUOTES: &[char] = &['$', '`'];

/// Characters that a backslash quotes in double quotes. Before each other
/// character, a backslash in double quotes is text.
const ESCAPED_IN_DOUBLE_QUOTES: &[char] = &['"', '\\', '$', '`'];

/// The words of `command`.
///
/// # Errors
///
/// Returns the reason if the command has no word, has a character that a
/// shell does not read as text, or has a quote without its end.
pub(crate) fn words(command: &str) -> Result<Vec<String>, WordsError> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut characters = command.chars();
    while let Some(character) = characters.next() {
        match character {
            ' ' | '\t' => words.extend(word.take()),
            '\'' => {
                let text = word.get_or_insert_default();
                loop {
                    match characters.next().ok_or(WordsError::Unterminated)? {
                        '\'' => break,
                        quoted => text.push(quoted),
                    }
                }
            }
            '"' => {
                let text = word.get_or_insert_default();
                loop {
                    match characters.next().ok_or(WordsError::Unterminated)? {
                        '"' => break,
                        '\\' => {
                            let next = characters.next().ok_or(WordsError::Unterminated)?;
                            if !ESCAPED_IN_DOUBLE_QUOTES.contains(&next) {
                                text.push('\\');
                            }
                            text.push(next);
                        }
                        expanded if EXPANDED_IN_DOUBLE_QUOTES.contains(&expanded) => {
                            return Err(WordsError::ShellCharacter(expanded));
                        }
                        quoted => text.push(quoted),
                    }
                }
            }
            '\\' => match characters.next().ok_or(WordsError::Unterminated)? {
                // A shell joins the two lines: the command is not one line.
                '\n' => return Err(WordsError::ShellCharacter('\n')),
                quoted => word.get_or_insert_default().push(quoted),
            },
            special if SHELL_CHARACTERS.contains(&special) => {
                return Err(WordsError::ShellCharacter(special));
            }
            text => word.get_or_insert_default().push(text),
        }
    }
    words.extend(word);
    if words.is_empty() {
        return Err(WordsError::Empty);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words_of(command: &str) -> Vec<String> {
        words(command).unwrap()
    }

    #[test]
    fn the_words_follow_the_quoting_of_a_shell() {
        assert_eq!(
            words_of("claude --resume 0b1c --model claude-haiku-4-5"),
            ["claude", "--resume", "0b1c", "--model", "claude-haiku-4-5"]
        );
        assert_eq!(
            words_of(r#"  tool  'a b' "c d" e\ f '' "g\"h" "i\j"  "#),
            ["tool", "a b", "c d", "e f", "", "g\"h", "i\\j"]
        );
        // Text in single quotes is text, also characters of a shell.
        assert_eq!(words_of("tool 'a;b|c$d`e'"), ["tool", "a;b|c$d`e"]);
        assert_eq!(words_of(r"tool a\;b"), ["tool", "a;b"]);
    }

    #[test]
    fn a_command_that_a_shell_reads_as_more_than_words_does_not_start() {
        let refused = [
            ("tool; rm -rf ~", WordsError::ShellCharacter(';')),
            ("tool && curl evil", WordsError::ShellCharacter('&')),
            ("tool | sh", WordsError::ShellCharacter('|')),
            ("tool > ~/.bashrc", WordsError::ShellCharacter('>')),
            ("tool < /etc/passwd", WordsError::ShellCharacter('<')),
            ("tool $(id)", WordsError::ShellCharacter('$')),
            ("tool `id`", WordsError::ShellCharacter('`')),
            ("tool \"$HOME\"", WordsError::ShellCharacter('$')),
            ("tool \"`id`\"", WordsError::ShellCharacter('`')),
            ("tool *", WordsError::ShellCharacter('*')),
            ("tool ?", WordsError::ShellCharacter('?')),
            ("tool [a]", WordsError::ShellCharacter('[')),
            ("tool ~/x", WordsError::ShellCharacter('~')),
            ("tool # comment", WordsError::ShellCharacter('#')),
            ("(tool)", WordsError::ShellCharacter('(')),
            ("tool\nrm -rf ~", WordsError::ShellCharacter('\n')),
            ("tool \\\nrm", WordsError::ShellCharacter('\n')),
            ("tool 'a", WordsError::Unterminated),
            ("tool \"a", WordsError::Unterminated),
            ("tool a\\", WordsError::Unterminated),
            ("", WordsError::Empty),
            ("   ", WordsError::Empty),
        ];
        for (command, error) in refused {
            assert_eq!(words(command), Err(error), "{command:?}");
        }
    }
}
