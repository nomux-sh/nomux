//! A host: the computer on which a daemon runs. A host is the local host
//! or a remote host that the client reaches through SSH. Each operation is
//! the same for the two.

use std::sync::Arc;

use crate::bootstrap::{BinarySource, BootstrapStep, Installation};
use crate::local_host::LocalHost;
use crate::ssh_host::SshHost;
use crate::terminal::{AttachOptions, Reattach, TerminalInput, TerminalOutput};
use crate::{Client, ClientError, ClientKind};

/// A host on which the client uses the daemon.
///
/// The value is a handle: a copy is the same host with the same SSH
/// connection.
#[derive(Debug, Clone)]
pub struct Host {
    place: Place,
}

#[derive(Debug, Clone)]
enum Place {
    Local(Arc<LocalHost>),
    Ssh(Arc<SshHost>),
}

impl Host {
    /// The local host, with the daemon of this user.
    ///
    /// # Errors
    ///
    /// Returns an error if the `HOME` variable is not set.
    pub fn local() -> Result<Self, ClientError> {
        Ok(Self {
            place: Place::Local(Arc::new(LocalHost::of_this_user()?)),
        })
    }

    /// Connects to a remote host through SSH and makes sure that the host
    /// has the nomux binaries of the version of this client. `ssh_target`
    /// is `[user@]host`, `ssh://[user@]host[:port]` or a `Host` name of
    /// `~/.ssh/config`. `progress` gets each step that can take time.
    ///
    /// The function starts no daemon: [`client`](Self::client) does.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::PermissionDenied`] if the host accepts no key
    /// of the user, [`ClientError::Connection`] if the host cannot be
    /// reached, and [`ClientError::Failed`] for each other reason: the host
    /// key, the SSH configuration, a host that nomux does not support, or
    /// binaries that cannot be installed.
    pub async fn connect_ssh(
        ssh_target: &str,
        binaries: &BinarySource,
        progress: &(dyn Fn(BootstrapStep<'_>) + Send + Sync),
    ) -> Result<(Self, Installation), ClientError> {
        let (host, installation) = SshHost::connect(ssh_target, binaries, progress).await?;
        let host = Self {
            place: Place::Ssh(Arc::new(host)),
        };
        Ok((host, installation))
    }

    /// Opens a connection to the daemon of the host. If no daemon runs,
    /// starts one and waits until it answers. On a remote host, a daemon
    /// that has no common protocol version with this client is replaced by
    /// the daemon of the version of this client. No session ends.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::DaemonStart`] if the daemon cannot start,
    /// [`ClientError::Timeout`] if it does not answer in time, and the
    /// errors of the connection.
    pub async fn client(&self, kind: ClientKind) -> Result<Client, ClientError> {
        match &self.place {
            Place::Local(local) => local.client(kind).await,
            Place::Ssh(ssh) => ssh.client(kind).await,
        }
    }

    /// Opens a connection to the daemon of the host. Returns `None` if no
    /// daemon runs: the function starts none.
    ///
    /// # Errors
    ///
    /// Returns the errors of the connection, and
    /// [`ClientError::VersionIncompatible`] if the daemon has no common
    /// protocol version with this client.
    pub async fn running_client(&self, kind: ClientKind) -> Result<Option<Client>, ClientError> {
        match &self.place {
            Place::Local(local) => local.running_client(kind).await,
            Place::Ssh(ssh) => ssh.running_client(kind).await,
        }
    }

    /// Attaches to the terminal of the session `id` on a connection of its
    /// own. If that connection ends, the attach connects again and
    /// continues from its position: [`TerminalOutput::next`] has the
    /// details.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`client`](Self::client) and of
    /// [`Client::attach`].
    pub async fn attach(
        &self,
        kind: ClientKind,
        id: &str,
        options: AttachOptions,
    ) -> Result<(TerminalOutput, TerminalInput), ClientError> {
        let attached = self.client(kind).await?.attach_frames(id, options).await?;
        let (input, new_input) = TerminalInput::reattaching(attached.input, attached.handle);
        let reattach = Reattach {
            host: self.clone(),
            kind,
            session_id: id.to_owned(),
            new_input,
        };
        let output = TerminalOutput::new(attached.output, attached.handle, options.resume)
            .reattaching(reattach);
        Ok((output, input))
    }
}
