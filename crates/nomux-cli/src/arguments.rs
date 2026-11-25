//! The command line of `nomux`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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

impl Arguments {
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
    Ls,
    /// Show a session in this terminal. To detach, press Ctrl-\ and then d
    #[command(after_help = ATTACH_KEYS)]
    Attach {
        /// The session identifier
        id: String,
    },
    /// Detach the clients that are attached to a session. The session continues
    Detach {
        /// The session identifier
        id: String,
    },
    /// End a session
    Kill {
        /// The session identifier
        id: String,
    },
    /// Show or stop the daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Add, list, remove and check the remote hosts
    Host {
        #[command(subcommand)]
        command: HostCommand,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The parts of a `new` command line: `--json`, `--attach`, and the
    /// command of the session.
    fn new_session(arguments: &[&str]) -> (bool, bool, Vec<String>) {
        let arguments = Arguments::try_parse_from([&["nomux", "new"], arguments].concat()).unwrap();
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
            new_session(&["--cmd", "sh", "--json", "--", "-c", "true"]),
            (false, false, words(&["sh", "--json", "--", "-c", "true"]))
        );
    }
}
