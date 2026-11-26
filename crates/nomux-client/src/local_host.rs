//! The local host: where the daemon of this user listens and how a client
//! starts it.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use nomux_core::{companion_binary, runtime_directory};
use nomux_transport::SOCKET_FILE;
use tokio::time::{Instant, sleep};

use crate::{Client, ClientError, ClientKind};

const DAEMON_BINARY: &str = "nomuxd";
/// The longest time from the start of the daemon to its first answer.
const START_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_INTERVAL: Duration = Duration::from_millis(20);

/// The local host, with the socket of the daemon of this user.
#[derive(Debug)]
pub(crate) struct LocalHost {
    socket: PathBuf,
}

impl LocalHost {
    pub(crate) fn of_this_user() -> Result<Self, ClientError> {
        let directory = runtime_directory::locate()
            .and_then(|directory| runtime_directory::link_socket_directory(&directory))
            .map_err(|error| ClientError::Failed(error.to_string()))?;
        Ok(Self {
            socket: directory.join(SOCKET_FILE),
        })
    }

    /// Connects to the daemon. Returns `None` if no daemon runs.
    pub(crate) async fn running_client(
        &self,
        kind: ClientKind,
    ) -> Result<Option<Client>, ClientError> {
        match Client::connect(&self.socket, kind).await {
            Ok(client) => Ok(Some(client)),
            Err(ClientError::DaemonNotRunning) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Connects to the daemon. If no daemon runs, starts one and waits
    /// until it answers.
    pub(crate) async fn client(&self, kind: ClientKind) -> Result<Client, ClientError> {
        match self.running_client(kind).await? {
            Some(client) => Ok(client),
            None => start(&self.socket, kind).await,
        }
    }
}

/// Starts a daemon that continues after this process ends, and connects to
/// it when it answers.
///
/// If a different process starts a daemon at the same time, only one of the
/// two daemons stays, and this function connects to that one.
async fn start(socket: &Path, kind: ClientKind) -> Result<Client, ClientError> {
    // `nomuxd --detach` starts the daemon as a separate process and ends.
    let status = Command::new(companion_binary(DAEMON_BINARY))
        .arg("--detach")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map_err(ClientError::DaemonStart)?;
    if !status.success() {
        return Err(ClientError::DaemonStart(io::Error::other(format!(
            "`{DAEMON_BINARY} --detach` ended with {status}"
        ))));
    }

    let deadline = Instant::now() + START_TIMEOUT;
    loop {
        match Client::connect(socket, kind).await {
            Err(ClientError::DaemonNotRunning) if Instant::now() < deadline => {
                sleep(CONNECT_INTERVAL).await;
            }
            Err(ClientError::DaemonNotRunning) => {
                return Err(ClientError::Timeout(format!(
                    "the daemon did not answer in {} seconds after its start",
                    START_TIMEOUT.as_secs()
                )));
            }
            result => return result,
        }
    }
}
