//! `nomux`: the command-line interface.
//!
//! # Responsibility
//!
//! The CLI reads the command line, calls the client library and prints the
//! result as text or as JSON. `docs/cli.md` has the commands, the JSON
//! schemas and the exit codes.
//!
//! The modules:
//!
//! - `arguments`: the command line.
//! - `commands`: one call of the client library for each command.
//! - `selected_host`: the host of a command: the local host, or the remote
//!   host of the `--host` option.
//! - `host_commands`: the `host` commands, which change and check the
//!   remote hosts of the global configuration file.
//! - `attach`: the `attach` command, which uses the terminal of the user.
//! - `report`: the result of a command.
//! - `output`: the text and the JSON form of a result and of an error.
//! - `exit_code`: the exit codes.
//!
//! # Boundaries
//!
//! - The CLI contains no protocol code. The `nomux-client` crate owns it.
//! - The CLI does not talk to a holder. It talks to the daemon only.
//! - No command asks the user a question. Only `attach` reads the terminal.
//!
//! # Status
//!
//! The CLI has the commands `new`, `ls`, `attach`, `detach`, `kill`,
//! `daemon status`, `daemon stop`, `host add`, `host ls`, `host rm`,
//! `host doctor` and `version`, and the `--host` option. The other commands
//! are not written yet.

mod arguments;
mod attach;
mod commands;
mod exit_code;
mod host_commands;
mod output;
mod report;
mod selected_host;

use std::io;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use crate::arguments::Arguments;
use crate::output::Format;

fn main() -> ExitCode {
    let arguments = match Arguments::try_parse() {
        Ok(arguments) => arguments,
        Err(error) => {
            // clap exits with status 2 for a usage error. That status means
            // "not found" here, thus a usage error has status 1.
            let code = if error.use_stderr() {
                exit_code::ERROR
            } else {
                exit_code::SUCCESS
            };
            return exit_after(error.print(), code);
        }
    };
    let format = Format::of(&arguments);
    let host = arguments.host.clone();
    let Some(command) = arguments.requested_command() else {
        let error =
            Arguments::command().error(ErrorKind::MissingSubcommand, "a command is necessary");
        return exit_after(error.print(), exit_code::ERROR);
    };

    match run(command, host.as_deref(), format) {
        Ok(report) => exit_after(output::print_report(&report, format), report.exit_code()),
        Err(error) => {
            let code = exit_code::of_error(&error);
            exit_after(output::print_error(&error, code, format), code)
        }
    }
}

fn run(
    command: arguments::Command,
    host: Option<&str>,
    format: Format,
) -> Result<report::Report, nomux_client::ClientError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(nomux_client::ClientError::Connection)?;
    runtime.block_on(commands::run(command, host, format))
}

/// The exit status is `code`, or the error status if the output failed, for
/// example because the reader closed the pipe.
fn exit_after(printed: io::Result<()>, code: u8) -> ExitCode {
    ExitCode::from(printed.map_or(exit_code::ERROR, |()| code))
}
