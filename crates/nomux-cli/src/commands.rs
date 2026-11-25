//! Runs one command: one connection to the daemon of the selected host and
//! one request.

use std::env;
use std::path::{self, PathBuf};

use nomux_client::{ClientError, ClientKind, Host, NewSession};

use crate::arguments::{Command, DaemonCommand};
use crate::output::Format;
use crate::report::Report;
use crate::{attach, extension_commands, host_commands, selected_host};

pub(crate) const KIND: ClientKind = ClientKind::Cli;

/// Runs `command` on the local host, or on the remote host `host_name` of
/// the configuration file.
pub(crate) async fn run(
    command: Command,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    match command {
        Command::Version => return Ok(Report::Version),
        Command::Host { .. } if host_name.is_some() => {
            return Err(ClientError::Failed(
                "the --host option cannot be used with a `host` command: give the name of the                  host as an argument"
                    .to_owned(),
            ));
        }
        Command::Host { command } => return host_commands::run(command, format).await,
        Command::Ext { command } => {
            return extension_commands::run(command, host_name, format).await;
        }
        Command::New { attach: true, .. } | Command::Attach { .. } => {
            // A session that no terminal can show does not start, and a
            // host is not contacted for it.
            attach::require_terminal()?;
        }
        _ => {}
    }
    let host = selected_host::connect(host_name, format).await?;
    match command {
        Command::New {
            cwd,
            attach,
            command,
            command_after_separator,
        } => {
            // The command line has the command in one of the two forms.
            let session = NewSession {
                command: [command, command_after_separator].concat(),
                working_directory: working_directory(cwd, host_name.is_some())?,
            };
            let session = host.client(KIND).await?.create_session(session).await?;
            if attach {
                attach_to(&host, host_name, session.id).await
            } else {
                Ok(Report::SessionCreated(session))
            }
        }
        Command::Attach { id } => attach_to(&host, host_name, id).await,
        Command::Detach { id } => {
            let clients = host.client(KIND).await?.detach_session(&id).await?;
            Ok(Report::ClientsDetached { id, clients })
        }
        Command::Ls => {
            let sessions = host.client(KIND).await?.list_sessions().await?;
            Ok(Report::Sessions(sessions))
        }
        // A session lives without a daemon, thus `kill` starts one too.
        Command::Kill { id } => {
            host.client(KIND).await?.kill_session(&id).await?;
            Ok(Report::SessionKilled { id })
        }
        Command::Daemon { command } => daemon(&host, command).await,
        Command::Host { .. } | Command::Ext { .. } | Command::Version => {
            unreachable!("handled before the connection")
        }
    }
}

async fn attach_to(
    host: &Host,
    host_name: Option<&str>,
    id: String,
) -> Result<Report, ClientError> {
    let end = attach::run(host, host_name, &id).await?;
    Ok(Report::AttachEnded { id, end })
}

/// The daemon commands start no daemon.
async fn daemon(host: &Host, command: DaemonCommand) -> Result<Report, ClientError> {
    let client = host.running_client(KIND).await?;
    match (command, client) {
        (DaemonCommand::Status, None) => Ok(Report::DaemonStatus(None)),
        (DaemonCommand::Status, Some(mut client)) => {
            let info = client.host_info().await?;
            Ok(Report::DaemonStatus(Some(info)))
        }
        (DaemonCommand::Stop, None) => Ok(Report::DaemonStopped { was_running: false }),
        (DaemonCommand::Stop, Some(client)) => {
            client.stop_daemon().await?;
            Ok(Report::DaemonStopped { was_running: true })
        }
    }
}

/// The absolute working directory of a new session. On the local host:
/// `--cwd`, or the current directory of the CLI. On a remote host: `--cwd`,
/// which must be an absolute path there, or `None` for the home directory.
fn working_directory(
    requested: Option<PathBuf>,
    on_remote_host: bool,
) -> Result<Option<PathBuf>, ClientError> {
    let directory = match (requested, on_remote_host) {
        (Some(directory), true) if directory.is_absolute() => return Ok(Some(directory)),
        (Some(directory), true) => {
            return Err(ClientError::Failed(format!(
                "with --host, --cwd must be an absolute path on the host: {} is not absolute",
                directory.display()
            )));
        }
        (None, true) => return Ok(None),
        (Some(directory), false) => path::absolute(directory),
        (None, false) => env::current_dir(),
    };
    directory.map(Some).map_err(|error| {
        ClientError::Failed(format!("the working directory is not valid: {error}"))
    })
}
