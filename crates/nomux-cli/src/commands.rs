//! Runs one command: one connection to the daemon of the selected host and
//! one request.

use std::env;
use std::path::{self, PathBuf};

use nomux_client::{ClientError, ClientKind, Host, NewSession};

use crate::arguments::{Command, DaemonCommand};
use crate::output::Format;
use crate::report::Report;
use crate::{
    attach, extension_commands, hook_commands, host_commands, selected_host, service_files,
    status_commands,
};

pub(crate) const KIND: ClientKind = ClientKind::Cli;

/// Runs `command` on the local host, or on the remote host `host_name` of
/// the configuration file.
pub(crate) async fn run(
    command: Command,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    let command = match without_connection(command, host_name, format).await {
        Ok(done) => return done,
        Err(command) => command,
    };
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
        Command::Ls { all, memory: false } => {
            let sessions = host.client(KIND).await?.list_sessions(all).await?;
            Ok(Report::Sessions(sessions))
        }
        Command::Ls { all, memory: true } => {
            let mut client = host.client(KIND).await?;
            let sessions = client.list_sessions(all).await?;
            let memory = client.session_memory().await?;
            Ok(Report::SessionsMeasured { sessions, memory })
        }
        Command::Start { id } => {
            let session = host.client(KIND).await?.start_session(&id).await?;
            Ok(Report::SessionStarted(session))
        }
        Command::Capture { id, lines } => {
            let output = host.client(KIND).await?.capture_lines(&id, lines).await?;
            Ok(Report::OutputCaptured { id, output })
        }
        Command::RestorePolicy { id, policy } => {
            let mut client = host.client(KIND).await?;
            client.set_restore_policy(&id, policy).await?;
            Ok(Report::RestorePolicySet { id, policy })
        }
        // A session lives without a daemon, thus `kill` starts one too.
        Command::Kill { id } => {
            host.client(KIND).await?.kill_session(&id).await?;
            Ok(Report::SessionKilled { id })
        }
        Command::Daemon { command } => daemon(&host, command).await,
        Command::Host { .. }
        | Command::Ext { .. }
        | Command::Hook { .. }
        | Command::Hooks { .. }
        | Command::Status { .. }
        | Command::Agent { .. }
        | Command::Events { .. }
        | Command::Version => {
            unreachable!("handled before the connection")
        }
    }
}

/// Runs a command that needs no connection to the selected host, or that
/// fails before it. Gives the command back if it needs the connection.
async fn without_connection(
    command: Command,
    host_name: Option<&str>,
    format: Format,
) -> Result<Result<Report, ClientError>, Command> {
    Ok(match command {
        Command::Version => Ok(Report::Version),
        Command::Host { .. } if host_name.is_some() => Err(ClientError::Failed(
            "the --host option cannot be used with a `host` command: give the name of the host \
             as an argument"
                .to_owned(),
        )),
        Command::Host { command } => host_commands::run(command, format).await,
        Command::Ext { command } => extension_commands::run(command, host_name, format).await,
        Command::Hook { tool, event } => Ok(hook_commands::hook(tool, event).await),
        Command::Hooks { command } => hook_commands::run(command, host_name).await,
        Command::Status {
            command: Some(command),
            ..
        } => status_commands::set(command, host_name).await,
        Command::Status {
            tree,
            command: None,
        } => status_commands::tree(host_name, tree, format).await,
        Command::Agent { command } => status_commands::agent(command, host_name, format).await,
        Command::Events { filter, .. } => {
            status_commands::follow_events(&filter, host_name, format).await
        }
        Command::Daemon {
            command: DaemonCommand::InstallService { dry_run },
        } => on_this_computer(host_name).and_then(|()| service_files::install(dry_run)),
        Command::Daemon {
            command: DaemonCommand::UninstallService { dry_run },
        } => on_this_computer(host_name).and_then(|()| service_files::uninstall(dry_run)),
        // A session that no terminal can show does not start, and a host is
        // not contacted for it.
        Command::New { attach: true, .. } | Command::Attach { .. } => {
            match attach::require_terminal() {
                Ok(()) => return Err(command),
                Err(error) => Err(error),
            }
        }
        _ => return Err(command),
    })
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
        (DaemonCommand::InstallService { .. } | DaemonCommand::UninstallService { .. }, _) => {
            unreachable!("handled before the connection")
        }
    }
}

/// A service file is a file of the computer of the CLI.
fn on_this_computer(host_name: Option<&str>) -> Result<(), ClientError> {
    match host_name {
        None => Ok(()),
        Some(_) => Err(ClientError::Failed(
            "the --host option cannot be used with this command: run it on the host".to_owned(),
        )),
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
