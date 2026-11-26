//! Runs one command: one connection to the daemon of the selected host and
//! one request.

use nomux_client::{ClientError, ClientKind, Host};
use nomux_core::configuration::default_configuration_path;

use crate::arguments::{Command, DaemonCommand};
use crate::output::Format;
use crate::report::Report;
use crate::{
    attach, extension_commands, hook_commands, host_commands, selected_host, service_files,
    session_commands, status_commands, workspace_commands,
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
        Err(command) => *command,
    };
    let host = selected_host::connect(host_name, format).await?;
    match command {
        Command::New(arguments) => session_commands::new(&host, host_name, arguments).await,
        Command::Attach(arguments) => session_commands::attach(&host, host_name, &arguments).await,
        Command::Detach { target } => session_commands::detach(&host, &target).await,
        Command::Ls { all, memory: false } => {
            let mut client = host.client(KIND).await?;
            let sessions = client.list_sessions(all).await?;
            let workspace = client.workspace().await?;
            Ok(Report::Sessions {
                sessions,
                workspace,
            })
        }
        Command::Ls { all, memory: true } => {
            let mut client = host.client(KIND).await?;
            let sessions = client.list_sessions(all).await?;
            let memory = client.session_memory().await?;
            Ok(Report::SessionsMeasured { sessions, memory })
        }
        Command::Start { target } => session_commands::start(&host, &target).await,
        Command::Capture(arguments) => session_commands::capture(&host, &arguments, format).await,
        Command::RestorePolicy { target, policy } => {
            session_commands::set_restore_policy(&host, &target, policy).await
        }
        Command::Kill(arguments) => session_commands::kill(&host, &arguments).await,
        Command::Send(arguments) => session_commands::send(&host, &arguments).await,
        Command::Rename(arguments) => session_commands::rename(&host, &arguments).await,
        Command::Daemon { command } => daemon(&host, host_name, command).await,
        Command::Host { .. }
        | Command::Ext { .. }
        | Command::Hook { .. }
        | Command::Hooks { .. }
        | Command::Status { .. }
        | Command::Agent { .. }
        | Command::Events { .. }
        | Command::Project { .. }
        | Command::Group { .. }
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
) -> Result<Result<Report, ClientError>, Box<Command>> {
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
        Command::Project { command } => {
            workspace_commands::project(command, host_name, format).await
        }
        Command::Group { command } => workspace_commands::group(command, host_name, format).await,
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
        Command::New(ref new) if new.attaches() => match attach::require_terminal() {
            Ok(()) => return Err(Box::new(command)),
            Err(error) => Err(error),
        },
        Command::Attach { .. } => match attach::require_terminal() {
            Ok(()) => return Err(Box::new(command)),
            Err(error) => Err(error),
        },
        _ => return Err(Box::new(command)),
    })
}

/// The daemon commands start no daemon.
async fn daemon(
    host: &Host,
    host_name: Option<&str>,
    command: DaemonCommand,
) -> Result<Report, ClientError> {
    let client = host.running_client(KIND).await?;
    // The CLI knows only the path of the local host.
    let configuration = host_name
        .is_none()
        .then(default_configuration_path)
        .flatten();
    match (command, client) {
        (DaemonCommand::Status, None) => Ok(Report::DaemonStatus {
            info: None,
            configuration,
        }),
        (DaemonCommand::Status, Some(mut client)) => {
            let info = client.host_info().await?;
            Ok(Report::DaemonStatus {
                info: Some(info),
                configuration,
            })
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
