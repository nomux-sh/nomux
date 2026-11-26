//! The command line of `nomux`.

use std::path::PathBuf;

use std::ffi::OsString;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};
use nomux_client::RestorePolicy;

use crate::extension_commands::ExtCommand;
use crate::status_commands::{AgentCommand, EventTypeArgument, StatusCommand};

/// Sessions that stay alive on the host when the client disconnects.
#[derive(Debug, Parser)]
#[command(
    name = "nomux",
    disable_version_flag = true,
    disable_help_subcommand = true
)]
pub(crate) struct Arguments {
    /// Print the result as one JSON document
    #[arg(long, global = true, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Print only the necessary part of the result
    #[arg(long, short, global = true)]
    pub(crate) quiet: bool,

    /// Use the daemon of a remote host of `nomux host ls` [default: the local host]
    #[arg(long, global = true, value_name = "NAME")]
    pub(crate) host: Option<String>,

    /// Print the version
    #[arg(long, short = 'V')]
    version: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

/// What ends the options of `nomux` in the second form of a command.
const SEPARATOR: &str = "--";

impl Arguments {
    /// Reads the command line `arguments`, with the name of the program
    /// first.
    ///
    /// # Errors
    ///
    /// Returns the usage error of a command line that is not valid.
    pub(crate) fn from_command_line(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, clap::Error> {
        let arguments = Self::try_parse_from(arguments)?;
        // `--cmd` takes each argument after it, thus the parser does not
        // see a separator there as the second form.
        if let Some(Command::New { command, .. }) = &arguments.command
            && command.iter().any(|word| word == SEPARATOR)
        {
            // The usage line of the message is the one of `new`.
            let mut nomux = Self::command();
            nomux.build();
            let new = nomux
                .find_subcommand_mut("new")
                .expect("the new command exists");
            return Err(new.error(
                ErrorKind::ArgumentConflict,
                "the argument '--cmd <COMMAND>...' cannot be used with '--': nomux cannot know \
                 where the command of the session starts. Give the command in one form: \
                 `nomux new --cmd <command> [argument...]` or \
                 `nomux new -- <command> [argument...]`. For a command that has `--` as an \
                 argument, use the second form",
            ));
        }
        Ok(arguments)
    }

    /// The command to run. `--version` is the `version` command. Returns
    /// `None` if the command line has no command.
    pub(crate) fn requested_command(self) -> Option<Command> {
        if self.version {
            Some(Command::Version)
        } else {
            self.command
        }
    }
}

const ATTACH_KEYS: &str = "\
Keys during an attach:
  Ctrl-\\ d       Detach. The session continues
  Ctrl-\\ Ctrl-\\  Send one Ctrl-\\ to the session

Each other key goes to the session. The exit code is the exit status of the
command if the session ends during the attach, and 0 after a detach.";

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Start a new session and print its identifier
    New {
        /// The working directory of the command [default: the current directory]
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,

        /// Attach to the new session
        #[arg(long)]
        attach: bool,

        /// The command of the session and its arguments [default: the shell of the user].
        /// Each argument after --cmd belongs to the command, also an option of nomux
        #[arg(
            long = "cmd",
            value_name = "COMMAND",
            num_args = 1..,
            allow_hyphen_values = true,
            conflicts_with = "command_after_separator"
        )]
        command: Vec<String>,

        /// The same as --cmd: each argument after -- belongs to the command
        #[arg(last = true, value_name = "COMMAND")]
        command_after_separator: Vec<String>,
    },
    /// List the sessions
    Ls {
        /// Also list the sessions whose command ended
        #[arg(long, short)]
        all: bool,

        /// Show the memory that the processes of each session use now
        #[arg(long)]
        memory: bool,
    },
    /// Show a session in this terminal. To detach, press Ctrl-\ and then d.
    /// A dormant session starts again
    #[command(after_help = ATTACH_KEYS)]
    Attach {
        /// The session identifier
        id: String,
    },
    /// Start the command of a dormant session again
    Start {
        /// The session identifier
        id: String,
    },
    /// Print the last output lines of a session
    Capture {
        /// The session identifier
        id: String,

        /// The number of lines
        #[arg(long, value_name = "N", default_value_t = 100)]
        lines: usize,
    },
    /// Set when a dormant session starts again
    RestorePolicy {
        /// The session identifier
        id: String,

        /// auto: when the daemon starts. on-focus: at the first attach.
        /// manual: only with `nomux start`
        policy: RestorePolicy,
    },
    /// Detach the clients that are attached to a session. The session continues
    Detach {
        /// The session identifier
        id: String,
    },
    /// End a session. A dormant session is removed with its output
    Kill {
        /// The session identifier
        id: String,
    },
    /// Show the status of each session: what works, what is done, what needs you
    #[command(args_conflicts_with_subcommands = true)]
    Status {
        /// Also show the Child work of each session
        #[arg(long)]
        tree: bool,

        #[command(subcommand)]
        command: Option<StatusCommand>,
    },
    /// Explain the status of the sessions
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Print the events of the host when they come, until a signal stops the command
    Events {
        /// Wait for the events. This is the only mode now
        #[arg(long, required = true)]
        follow: bool,

        /// Print only the events of this type. Give the option more than one time for more
        /// types
        #[arg(long, value_name = "TYPE")]
        filter: Vec<EventTypeArgument>,
    },
    /// Show or stop the daemon, and start it at login
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Add, list, remove and check the remote hosts
    Host {
        #[command(subcommand)]
        command: HostCommand,
    },
    /// Install, list, remove and call the extensions of a host
    Ext {
        #[command(subcommand)]
        command: ExtCommand,
    },
    /// Print the version
    Version,
}

#[derive(Debug, Subcommand)]
pub(crate) enum HostCommand {
    /// Connect to a host through SSH, install nomux there and save the host
    Add {
        /// The SSH destination: [user@]host, ssh://[user@]host[:port] or a
        /// Host name of ~/.ssh/config
        #[arg(value_name = "SSH-TARGET")]
        target: String,

        /// The name of the host for --host [default: the host part of the destination]
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
    /// List the hosts of the configuration file
    Ls,
    /// Remove a host from the configuration file. Nothing changes on the host
    Rm {
        /// The name of the host
        name: String,
    },
    /// Check the connection, the platform, the binaries, the permissions and the daemon of a host
    Doctor {
        /// The name of the host [default: each host of the configuration file]
        name: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub(crate) enum DaemonCommand {
    /// Show if the daemon runs. The exit status is 2 if it does not run
    Status,
    /// Stop the daemon. The sessions continue
    Stop,
    /// Write the file that starts the daemon at login: a systemd user unit
    /// on Linux, a launchd agent on macOS
    InstallService {
        /// Print the path and the content of the file, and write nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the file that starts the daemon at login
    UninstallService {
        /// Print the path of the file, and remove nothing
        #[arg(long)]
        dry_run: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts of a `new` command line: `--json`, `--attach`, and the
    /// command of the session.
    fn new_session(arguments: &[&str]) -> (bool, bool, Vec<String>) {
        let arguments = command_line(&[&["new"], arguments].concat()).unwrap();
        let json = arguments.json;
        let Some(Command::New {
            attach,
            command,
            command_after_separator,
            ..
        }) = arguments.requested_command()
        else {
            panic!("not the new command");
        };
        (json, attach, [command, command_after_separator].concat())
    }

    fn command_line(arguments: &[&str]) -> Result<Arguments, clap::Error> {
        let program = std::iter::once("nomux");
        Arguments::from_command_line(program.chain(arguments.iter().copied()).map(OsString::from))
    }

    fn words(words: &[&str]) -> Vec<String> {
        words.iter().map(|&word| word.to_owned()).collect()
    }

    #[test]
    fn the_options_before_the_separator_belong_to_nomux() {
        assert_eq!(
            new_session(&["--json", "--attach", "--", "sleep", "300"]),
            (true, true, words(&["sleep", "300"]))
        );
        assert_eq!(
            new_session(&["--json", "--cmd", "sleep", "300"]),
            (true, false, words(&["sleep", "300"]))
        );
    }

    #[test]
    fn each_argument_after_the_command_starts_belongs_to_the_command() {
        assert_eq!(
            new_session(&["--", "sleep", "300", "--json", "--attach"]),
            (false, false, words(&["sleep", "300", "--json", "--attach"]))
        );
        assert_eq!(
            new_session(&["--cmd", "sh", "--json", "-c", "true"]),
            (false, false, words(&["sh", "--json", "-c", "true"]))
        );
        // After the separator, a second separator is an argument of the
        // command.
        assert_eq!(
            new_session(&["--", "git", "log", "--", "notes.txt"]),
            (false, false, words(&["git", "log", "--", "notes.txt"]))
        );
    }

    #[test]
    fn the_two_forms_of_the_command_in_one_command_line_are_a_usage_error() {
        for arguments in [
            &["new", "--cmd", "sh", "--", "-c", "true"][..],
            &["new", "--cmd", "sh", "--"],
            &["new", "--attach", "--cmd", "--", "sh"],
        ] {
            let error = command_line(arguments).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{arguments:?}");
            assert!(error.use_stderr());
            let message = error.to_string();
            assert!(
                message.contains("'--cmd <COMMAND>...' cannot be used with '--'"),
                "{message}"
            );
        }
        // The rule is for `new` only: a different command has no `--cmd`.
        assert!(command_line(&["ext", "call", "a.b", "m", "--payload", "--"]).is_ok());
    }
}
