//! A host: the computer on which a daemon runs. A host is the local host
//! or a remote host that the client reaches through SSH. Each operation is
//! the same for the two.

use std::sync::Arc;

use std::path::PathBuf;

use crate::bootstrap::{BinarySource, BootstrapStep, Installation, RemotePlatform};
use crate::extension_archive::{ExtensionArchive, downloads_directory};
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

/// The package of an archive on a host, before the daemon installs it.
#[derive(Debug)]
pub struct StagedPackage {
    /// The package directory: a path on the host.
    pub directory: PathBuf,
    root: StagedRoot,
}

#[derive(Debug)]
enum StagedRoot {
    Local(PathBuf),
    /// A path below the home directory of the user on the host.
    Remote(String),
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

    /// The release target of the host, for example
    /// `x86_64-unknown-linux-musl`: the name of a release archive for the
    /// host ends with it.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if nomux has no release target for
    /// this computer.
    pub fn release_target(&self) -> Result<String, ClientError> {
        match &self.place {
            Place::Local(_) => Ok(RemotePlatform::of_this_computer()?.target.to_owned()),
            Place::Ssh(ssh) => Ok(ssh.release_target().to_owned()),
        }
    }

    /// Writes the package of `archive` on the host, into a new directory
    /// of `~/.nomux/downloads` that only the user can read. Call it only
    /// after the user accepted the extension.
    ///
    /// # Errors
    ///
    /// Returns an error if a file cannot be written on the host.
    pub async fn stage_extension_package(
        &self,
        archive: &ExtensionArchive,
    ) -> Result<StagedPackage, ClientError> {
        match &self.place {
            Place::Local(_) => {
                let parent = downloads_directory()?;
                let root = tempfile::Builder::new()
                    .prefix("package-")
                    .tempdir_in(&parent)
                    .map_err(|error| ClientError::Failed(format!("{}: {error}", parent.display())))?
                    .keep();
                let directory = archive.write_package(&root).map_err(|error| {
                    let _ = std::fs::remove_dir_all(&root);
                    ClientError::Failed(format!("the package cannot be written: {error}"))
                })?;
                Ok(StagedPackage {
                    directory,
                    root: StagedRoot::Local(root),
                })
            }
            Place::Ssh(ssh) => {
                let (root, directory) = ssh.stage_package(archive).await?;
                Ok(StagedPackage {
                    directory: PathBuf::from(directory),
                    root: StagedRoot::Remote(root),
                })
            }
        }
    }

    /// Removes a package that [`stage_extension_package`] wrote. The daemon
    /// keeps a copy of an installed package.
    ///
    /// [`stage_extension_package`]: Self::stage_extension_package
    pub async fn remove_staged_package(&self, staged: StagedPackage) {
        match (&self.place, staged.root) {
            (_, StagedRoot::Local(root)) => {
                let _ = std::fs::remove_dir_all(root);
            }
            (Place::Ssh(ssh), StagedRoot::Remote(root)) => {
                ssh.remove_staged(&root).await;
            }
            _ => {}
        }
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
