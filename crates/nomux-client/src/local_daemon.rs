//! The daemon of this user on the local host: where it listens and how a
//! client starts it.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use nomux_core::companion_binary;
use tokio::time::{Instant, sleep};

use crate::{Client, ClientError, ClientKind};

const DAEMON_BINARY: &str = "nomuxd";
/// The longest time from the start of the daemon to its first answer.
const START_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_INTERVAL: Duration = Duration::from_millis(20);

pub(crate) fn socket_path() -> Result<PathBuf, ClientError> {
    nomux_transport::default_socket_path().ok_or_else(|| {
        ClientError::Failed(
            "the HOME variable is not set, thus the daemon socket has no path".to_owned(),
        )
    })
}

/// Starts a daemon that continues after this process ends, and connects to
/// it when it answers.
///
/// If a different process starts a daemon at the same time, only one of the
/// two daemons stays, and this function connects to that one.
pub(crate) async fn start(socket: &Path, kind: ClientKind) -> Result<Client, ClientError> {
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
