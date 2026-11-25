//! Runs one command: one connection to the daemon and one request.

use std::env;
use std::path::{self, PathBuf};

use nomux_client::{Client, ClientError, ClientKind, NewSession};

use crate::arguments::{Command, DaemonCommand};
use crate::report::Report;

const KIND: ClientKind = ClientKind::Cli;

pub(crate) async fn run(command: Command) -> Result<Report, ClientError> {
    match command {
        Command::New { cwd, command } => {
            let session = NewSession {
                command,
                working_directory: Some(working_directory(cwd)?),
            };
            let mut client = Client::connect_local_or_start(KIND).await?;
            client
                .create_session(session)
                .await
                .map(Report::SessionCreated)
        }
        Command::Ls => {
            let mut client = Client::connect_local_or_start(KIND).await?;
            client.list_sessions().await.map(Report::Sessions)
        }
        // A session lives without a daemon, thus `kill` starts one too.
        Command::Kill { id } => {
            let mut client = Client::connect_local_or_start(KIND).await?;
            client.kill_session(&id).await?;
            Ok(Report::SessionKilled { id })
        }
        Command::Daemon { command } => daemon(command).await,
        Command::Version => Ok(Report::Version),
    }
}

/// The daemon commands start no daemon.
async fn daemon(command: DaemonCommand) -> Result<Report, ClientError> {
    let client = match Client::connect_local(KIND).await {
        Ok(client) => Some(client),
        Err(ClientError::DaemonNotRunning) => None,
        Err(error) => return Err(error),
    };
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

/// The absolute working directory of a new session: `--cwd`, or the current
/// directory of the CLI.
fn working_directory(requested: Option<PathBuf>) -> Result<PathBuf, ClientError> {
    let directory = match requested {
        Some(directory) => path::absolute(directory),
        None => env::current_dir(),
    };
    directory.map_err(|error| {
        ClientError::Failed(format!("the working directory is not valid: {error}"))
    })
}
