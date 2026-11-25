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

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Start a new session and print its identifier
    New {
        /// The working directory of the command [default: the current directory]
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,

        /// The command of the session and its arguments [default: the shell of the user].
        /// All the arguments after --cmd belong to the command
        #[arg(
            long = "cmd",
            value_name = "COMMAND",
            num_args = 1..,
            allow_hyphen_values = true
        )]
        command: Vec<String>,
    },
    /// List the sessions
    Ls,
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
    /// Print the version
    Version,
}

#[derive(Debug, Clone, Copy, Subcommand)]
pub(crate) enum DaemonCommand {
    /// Show if the daemon runs. The exit status is 2 if it does not run
    Status,
    /// Stop the daemon. The sessions continue
    Stop,
}
