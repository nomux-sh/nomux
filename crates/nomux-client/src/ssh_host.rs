//! A remote host: one SSH connection, on which each connection to the
//! daemon is a channel, and the start of the daemon on the host.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use nomux_transport::EnvelopeStream;
use nomux_transport::ssh::{SshConnection, SshEnvironment, SshError, SshTarget};
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep};

use crate::bootstrap::{self, BinarySource, BootstrapStep, Installation, remote_shell};
use crate::{Client, ClientError, ClientKind};

/// The longest time from the start of the daemon to its first answer.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// A daemon that replaces a daemon waits until that daemon answered its
/// requests and stopped.
const REPLACE_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_INTERVAL: Duration = Duration::from_millis(50);

/// A remote host that has the nomux binaries.
#[derive(Debug)]
pub(crate) struct SshHost {
    target: SshTarget,
    environment: SshEnvironment,
    installation: Installation,
    /// The SSH connection. One task at a time connects again, thus a host
    /// that does not answer gets one connection attempt at a time.
    connection: Mutex<Arc<SshConnection>>,
}

impl SshHost {
    pub(crate) async fn connect(
        ssh_target: &str,
        binaries: &BinarySource,
        progress: &(dyn Fn(BootstrapStep<'_>) + Send + Sync),
    ) -> Result<(Self, Installation), ClientError> {
        let target: SshTarget = ssh_target.parse()?;
        let environment = SshEnvironment::of_process()?;
        let connection = SshConnection::connect(&target, &environment).await?;
        let installation = bootstrap::ensure_installed(&connection, binaries, progress).await?;
        let host = Self {
            target,
            environment,
            installation: installation.clone(),
            connection: Mutex::new(Arc::new(connection)),
        };
        Ok((host, installation))
    }

    pub(crate) async fn running_client(
        &self,
        kind: ClientKind,
    ) -> Result<Option<Client>, ClientError> {
        match self.open().await? {
            Some(stream) => Client::on(stream, kind).await.map(Some),
            None => Ok(None),
        }
    }

    pub(crate) async fn client(&self, kind: ClientKind) -> Result<Client, ClientError> {
        match self.running_client(kind).await {
            Ok(Some(client)) => Ok(client),
            Ok(None) => {
                self.start_daemon("--detach").await?;
                self.wait_for_daemon(kind, START_TIMEOUT).await
            }
            Err(ClientError::VersionIncompatible(_)) => {
                self.start_daemon("--detach --replace").await?;
                self.wait_for_daemon(kind, REPLACE_TIMEOUT).await
            }
            Err(error) => Err(error),
        }
    }

    /// Opens a channel to the socket of the daemon. Returns `None` if no
    /// daemon listens.
    async fn open(&self) -> Result<Option<EnvelopeStream>, ClientError> {
        let connection = self.connection().await?;
        match connection
            .open_unix_socket(&self.installation.daemon_socket)
            .await
        {
            Ok(stream) => Ok(Some(stream)),
            Err(SshError::SocketUnavailable) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// The SSH connection. Makes a new one if the connection ended.
    async fn connection(&self) -> Result<Arc<SshConnection>, ClientError> {
        let mut connection = self.connection.lock().await;
        if connection.is_closed() {
            *connection = Arc::new(SshConnection::connect(&self.target, &self.environment).await?);
        }
        Ok(Arc::clone(&connection))
    }

    /// Runs `nomuxd` of the version of this client on the host. The command
    /// ends when the daemon runs as a separate process.
    async fn start_daemon(&self, options: &str) -> Result<(), ClientError> {
        let command = remote_shell(&format!(
            "exec \"$HOME/.nomux/bin/{}/nomuxd\" {options}",
            self.installation.label
        ));
        let output = self.connection().await?.run(&command, &[]).await?;
        if output.succeeded() {
            return Ok(());
        }
        let reason = String::from_utf8_lossy(&output.stderr);
        Err(ClientError::DaemonStart(io::Error::other(format!(
            "`nomuxd {options}` failed on the host: {}",
            reason.trim()
        ))))
    }

    /// Connects until the daemon of this client answers. During a
    /// replacement, the earlier daemon can still answer.
    async fn wait_for_daemon(
        &self,
        kind: ClientKind,
        limit: Duration,
    ) -> Result<Client, ClientError> {
        let deadline = Instant::now() + limit;
        loop {
            let not_ready = match self.running_client(kind).await {
                Ok(Some(client)) => return Ok(client),
                Ok(None) => None,
                Err(error @ ClientError::VersionIncompatible(_)) => Some(error),
                Err(error) => return Err(error),
            };
            if Instant::now() < deadline {
                sleep(CONNECT_INTERVAL).await;
                continue;
            }
            if let Some(error) = not_ready {
                return Err(error);
            }
            let connection = self.connection().await?;
            return Err(if daemon_process_runs(&connection).await? {
                SshError::ForwardingRefused.into()
            } else {
                ClientError::Timeout(format!(
                    "the daemon on the host did not answer in {} seconds after its start. Its \
                     log is ~/.nomux/run/nomuxd.log on the host",
                    limit.as_secs()
                ))
            });
        }
    }
}

/// Returns `true` if the socket of the daemon exists on the host and the
/// process of the lock file runs.
///
/// OpenSSH answers a channel to a Unix socket in the same way when nothing
/// listens on the socket and when the server does not permit the channel.
/// A daemon that runs and cannot be reached means the second.
pub(crate) async fn daemon_process_runs(connection: &SshConnection) -> Result<bool, SshError> {
    let script = "d=\"$HOME/.nomux/run\"; p=$(cat \"$d/nomuxd.lock\" 2>/dev/null); \
                  [ -S \"$d/nomuxd.sock\" ] && [ -n \"$p\" ] && kill -0 \"$p\" 2>/dev/null";
    Ok(connection
        .run(&remote_shell(script), &[])
        .await?
        .succeeded())
}
