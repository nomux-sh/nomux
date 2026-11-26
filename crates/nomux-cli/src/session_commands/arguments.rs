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

/// The session of a command that has a default session: the most recent
/// one. The same forms as [`Target`].
#[derive(Debug, Args)]
pub(crate) struct OptionalTarget {
    /// The session: its identifier, its name, or the start of one of them [default: the session
    /// that a client attached to last, else the session that started last]
    #[arg(value_name = "TARGET")]
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

impl OptionalTarget {
    /// The text that the user gave, if any.
    pub(crate) fn text(&self) -> Option<&str> {
        self.target.as_deref().or(self.target_option.as_deref())
    }
}

/// The options of `attach`.
#[derive(Debug, Args)]
pub(crate) struct AttachArguments {
    #[command(flatten)]
    pub(crate) target: OptionalTarget,

    /// Detach the other clients of the session first
    #[arg(long = "detach-others", short = 'd')]
    pub(crate) detach_others: bool,

    /// Show the session and send no key and no window size to it. The prefix keys work
    #[arg(long, short)]
    pub(crate) read_only: bool,

    #[command(flatten)]
    pub(crate) prefix: PrefixOption,

    /// Start a session if the host has none: bare `nomux` on a terminal.
    #[arg(skip)]
    pub(crate) start_if_none: bool,
}

impl AttachArguments {
    /// The attach of bare `nomux`: to the most recent session, or to a new
    /// session.
    pub(crate) fn of_bare_command() -> Self {
        Self {
            target: OptionalTarget {
                target: None,
                target_option: None,
            },
            detach_others: false,
            read_only: false,
            prefix: PrefixOption::default(),
            start_if_none: true,
        }
    }
}

/// The options of `capture`.
#[derive(Debug, Args)]
pub(crate) struct CaptureArguments {
    #[command(flatten)]
    pub(crate) target: Target,

    /// The number of lines
    #[arg(long, value_name = "N", default_value_t = 100)]
    pub(crate) lines: usize,

    /// After the lines, print each byte that the command writes, until the session ends or a
    /// signal stops the command. Not with --json
    #[arg(long, short)]
    pub(crate) follow: bool,
}

/// The options of `kill`.
#[derive(Debug, Args)]
pub(crate) struct KillArguments {
    #[command(flatten)]
    pub(crate) target: OptionalTarget,

    /// End each session whose command runs. On a terminal, the command asks first
    #[arg(long, conflicts_with_all = ["target", "target_option"])]
    pub(crate) all: bool,

    #[command(flatten)]
    pub(crate) all_options: KillAllOptions,
}

/// The options of `kill --all`.
#[derive(Debug, Args)]
pub(crate) struct KillAllOptions {
    /// With --all, also remove the dormant and the damaged sessions with their output
    #[arg(long, requires = "all")]
    pub(crate) include_dormant: bool,

    /// With --all, print the sessions and end none
    #[arg(long, requires = "all")]
    pub(crate) dry_run: bool,

    /// With --all, do not ask. Without a terminal, --all needs it
    #[arg(long, requires = "all")]
    pub(crate) yes: bool,
}

/// The arguments of `send`.
#[derive(Debug, Args)]
pub(crate) struct SendArguments {
    /// The session, and then the text: its words with one space between them. With -t, only
    /// the text
    #[arg(
        value_name = "TARGET TEXT",
        num_args = 1..,
        required = true,
        allow_hyphen_values = true
    )]
    words: Vec<String>,

    /// The session
    #[arg(short = 't', long = "target", value_name = "TARGET", hide = true)]
    target_option: Option<String>,

    /// Do not press Enter after the text
    #[arg(long)]
    pub(crate) no_enter: bool,

    /// The words are key names, for example C-c, Enter, Up or F5. No Enter follows
    #[arg(long, short)]
    pub(crate) keys: bool,
}

impl SendArguments {
    /// The session and the words to send. `None` if no word is left for
    /// the text.
    pub(crate) fn target_and_words(&self) -> Option<(&str, &[String])> {
        match (self.target_option.as_deref(), self.words.as_slice()) {
            (Some(target), words) if !words.is_empty() => Some((target, words)),
            (None, [target, words @ ..]) if !words.is_empty() => Some((target, words)),
            _ => None,
        }
    }
}
