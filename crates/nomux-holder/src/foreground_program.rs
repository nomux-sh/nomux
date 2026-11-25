//! The program in the foreground of the terminal, and the rule that says if
//! an `app` value of a program status report agrees with it.
//!
//! Each program that writes to the terminal can send a report with each
//! `app` value. The holder accepts a report only if its `app` value agrees
//! with the program that the user works with at that moment: the leader of
//! the foreground process group of the terminal. `docs/holder.md` has the
//! limits of this rule.

use std::cell::OnceCell;
use std::os::fd::BorrowedFd;

use nomux_holder_protocol::discovery::program_name;
use rustix::termios::tcgetpgrp;

/// The characters that can follow the shorter name in the longer name.
const NAME_SEPARATORS: [char; 4] = ['-', '_', '.', '+'];

/// The program in the foreground of a terminal. The first question reads
/// its name from the operating system, and the next questions use that
/// answer: one chunk of output causes one lookup at most.
pub(crate) struct ForegroundProgram<'terminal> {
    terminal: BorrowedFd<'terminal>,
    /// The result of the lookup. `None` if the lookup found no name.
    name: OnceCell<Option<String>>,
}

impl<'terminal> ForegroundProgram<'terminal> {
    /// `terminal` is the master side of the pseudoterminal.
    pub(crate) fn of(terminal: BorrowedFd<'terminal>) -> Self {
        Self {
            terminal,
            name: OnceCell::new(),
        }
    }

    /// `true` if the name of the foreground program agrees with `app`.
    pub(crate) fn agrees_with(&self, app: &str) -> bool {
        let name = self.name.get_or_init(|| lookup(self.terminal));
        name.as_deref()
            .is_some_and(|program| names_agree(app, program))
    }
}

/// The program name of the leader of the foreground process group. `None`
/// if the terminal has no foreground process group, or if its leader ended.
fn lookup(terminal: BorrowedFd<'_>) -> Option<String> {
    let group = tcgetpgrp(terminal).ok()?;
    let leader = u32::try_from(group.as_raw_nonzero().get()).ok()?;
    program_name(leader).ok().flatten()
}

/// The two names agree if they are equal, or if one of them is the other
/// one with a separator and more text after it: `claude-code` agrees with
/// `claude`. Upper case and lower case ASCII letters are equal.
fn names_agree(app: &str, program: &str) -> bool {
    let app = app.to_ascii_lowercase();
    let program = program.to_ascii_lowercase();
    !program.is_empty() && (extends(&app, &program) || extends(&program, &app))
}

fn extends(longer: &str, shorter: &str) -> bool {
    longer
        .strip_prefix(shorter)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(NAME_SEPARATORS))
}

#[cfg(test)]
mod tests {
    use super::names_agree;

    #[test]
    fn a_name_agrees_with_the_same_name_and_with_its_extension_at_a_separator() {
        assert!(names_agree("codex", "codex"));
        assert!(names_agree("claude-code", "claude"));
        assert!(names_agree("terraform", "terraform.bin"));
        assert!(names_agree("Cargo", "cargo"));
    }

    #[test]
    fn a_name_that_only_starts_with_the_other_name_does_not_agree() {
        assert!(!names_agree("claude-code", "cat"));
        assert!(!names_agree("claudette", "claude"));
        assert!(!names_agree("sh", "ssh"));
        assert!(!names_agree("-rf", ""));
    }
}
