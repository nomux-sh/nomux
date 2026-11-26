//! The command line of the session commands.

use std::path::PathBuf;

use clap::Args;

use crate::attach::PrefixOption;

/// The session of a command: its identifier, its name, or a unique prefix of
/// one of them. `-t` is the same as the argument, for the habit of tmux.
#[derive(Debug, Args)]
pub(crate) struct Target {
    /// The session: its identifier, its name, or the start of one of them
    #[arg(value_name = "TARGET", required_unless_present = "target_option")]
    target: Option<String>,

    /// The same as TARGET
    #[arg(
        short = 't',
        long = "target",
        value_name = "TARGET",
        hide = true,
        conflicts_with = "target"
    )]
    target_option: Option<String>,
}

impl Target {
    /// The text that the user gave.
    pub(crate) fn text(&self) -> &str {
        self.target
            .as_deref()
            .or(self.target_option.as_deref())
            .expect("the parser requires one of the two forms")
    }
}

/// The options of `new`.
#[derive(Debug, Args)]
pub(crate) struct NewArguments {
    /// The name of the session: 1 to 64 characters from A-Z, a-z, 0-9, '.', '_' and '-', that
    /// start with a letter or a digit. No other session that runs has it
    #[arg(long, short, value_name = "NAME")]
    pub(crate) name: Option<String>,

    /// The working directory of the command [default: the current directory]
    #[arg(long, short = 'c', value_name = "DIR")]
    pub(crate) cwd: Option<PathBuf>,

    /// Attach to the new session
    #[arg(long, short, conflicts_with = "attach_or_create")]
    pub(crate) attach: bool,

    /// Attach to the session that has the name of --name. If no session has it, start the
    /// session and attach to it
    #[arg(long, short = 'A', requires = "name")]
    pub(crate) attach_or_create: bool,

    #[command(flatten)]
    pub(crate) prefix: PrefixOption,

    /// The command of the session and its arguments [default: the shell of the user].
    /// Each argument after --cmd belongs to the command, also an option of nomux
    #[arg(
        long = "cmd",
        value_name = "COMMAND",
        num_args = 1..,
        allow_hyphen_values = true,
        conflicts_with = "command_after_separator"
    )]
    pub(crate) command: Vec<String>,

    /// The same as --cmd: each argument after -- belongs to the command
    #[arg(last = true, value_name = "COMMAND")]
    pub(crate) command_after_separator: Vec<String>,
}

impl NewArguments {
    /// `true` if the command attaches to the session.
    pub(crate) fn attaches(&self) -> bool {
        self.attach || self.attach_or_create
    }

    /// The command of the session, from the one of the two forms that the
    /// command line has.
    pub(crate) fn command(&self) -> Vec<String> {
        [self.command.as_slice(), &self.command_after_separator].concat()
    }
}

/// The arguments of `rename`.
#[derive(Debug, Args)]
pub(crate) struct RenameArguments {
    /// The session, and then its new name. With -t, only the new name
    #[arg(value_name = "TARGET NEW-NAME", num_args = 1..=2, required = true)]
    words: Vec<String>,

    /// The session
    #[arg(short = 't', long = "target", value_name = "TARGET", hide = true)]
    target_option: Option<String>,
}

impl RenameArguments {
    /// The session and its new name. `None` if the command line has more or
    /// fewer words than its form needs.
    pub(crate) fn target_and_name(&self) -> Option<(&str, &str)> {
        match (self.target_option.as_deref(), self.words.as_slice()) {
            (Some(target), [name]) => Some((target, name)),
            (None, [target, name]) => Some((target, name)),
            _ => None,
        }
    }
}
