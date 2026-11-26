//! The bootstrap of a remote host: the client finds the operating system
//! and the processor of the host and, if the host does not have them,
//! installs the three nomux binaries of its own version.
//!
//! The host needs a POSIX shell and no other program: the client downloads
//! the release archive and sends the binaries through the SSH connection.
//! The binaries go into `~/.nomux/bin/<label>` with one rename, after the
//! client compared their SHA-256 checksums. `docs/remote.md` has the
//! details.

mod binaries;
mod install;
mod platform;
mod probe;
mod release_archive;

use std::path::Path;

use nomux_core::runtime_directory;
use nomux_transport::SOCKET_FILE;
use nomux_transport::ssh::{CommandOutput, SshConnection};

pub use self::binaries::BinarySource;
pub(crate) use self::binaries::sha256_hex;
use self::binaries::{Binaries, read_directory};
pub(crate) use self::platform::RemotePlatform;
pub(crate) use self::probe::probe;
use crate::ClientError;

/// The version of this client. The three binaries of an installation have
/// the same version.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The nomux binaries on a remote host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installation {
    /// The operating system of the host, as `uname -s` gives it.
    pub system: String,
    /// The processor of the host, as `uname -m` gives it.
    pub machine: String,
    /// The release target of the host, for example
    /// `x86_64-unknown-linux-musl`.
    pub target: String,
    /// The name of the directory of the binaries in `~/.nomux/bin` on the
    /// host: the version, and a checksum if the binaries are not from a
    /// release.
    pub label: String,
    /// The path of the socket of the daemon on the host.
    pub daemon_socket: String,
    /// `true` if this connection installed the binaries. `false` if the
    /// host already had them.
    pub installed_now: bool,
}

/// A step of the bootstrap that can take time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStep<'a> {
    /// The client downloads the release archive.
    Downloading {
        /// The address of the archive.
        url: &'a str,
    },
    /// The client sends the binaries to the host.
    Uploading {
        /// The release target of the host.
        target: &'a str,
        /// The number of bytes of the three binaries.
        bytes: usize,
    },
}

/// Makes sure that the host of `connection` has the binaries of this
/// client.
pub(crate) async fn ensure_installed(
    connection: &SshConnection,
    source: &BinarySource,
    progress: &(dyn Fn(BootstrapStep<'_>) + Send + Sync),
) -> Result<Installation, ClientError> {
    /// The binaries, or where the client gets them when the host needs
    /// them.
    enum Pending<'a> {
        Ready(Binaries),
        Download(&'a str),
    }
    let (label, pending) = match source {
        BinarySource::Directory(directory) => {
            let binaries = read_directory(directory)?;
            (binaries.label(), Pending::Ready(binaries))
        }
        BinarySource::Release(releases_url) => {
            (VERSION.to_owned(), Pending::Download(releases_url))
        }
    };
    let probe = probe(connection, &label).await?;
    let platform = RemotePlatform::from_uname(&probe.system, &probe.machine)?;
    if !probe.installed {
        let binaries = match pending {
            Pending::Ready(binaries) => binaries,
            Pending::Download(releases_url) => {
                release_archive::download(releases_url, platform.target, progress).await?
            }
        };
        progress(BootstrapStep::Uploading {
            target: platform.target,
            bytes: binaries.total_size(),
        });
        install::install(connection, &label, &binaries).await?;
    }
    Ok(Installation {
        target: platform.target.to_owned(),
        daemon_socket: daemon_socket(&probe.home),
        installed_now: !probe.installed,
        system: probe.system,
        machine: probe.machine,
        label,
    })
}

/// The socket of the daemon of a host whose home directory is `home`. The
/// rule of `nomux_core::runtime_directory` is the same on each host, thus
/// the path needs no other fact of the host.
pub(crate) fn daemon_socket(home: &str) -> String {
    let runtime_directory = runtime_directory::home_runtime_directory(Path::new(home));
    runtime_directory::socket_directory(&runtime_directory)
        .join(SOCKET_FILE)
        .to_string_lossy()
        .into_owned()
}

/// Wraps a script for the POSIX shell of the host. The login shell of the
/// user reads only one word in single quotes, thus the script means the
/// same with each login shell.
pub(crate) fn remote_shell(script: &str) -> String {
    debug_assert!(!script.contains('\''), "a script has no single quote");
    format!("sh -c '{script}'")
}

/// The error for a script that did not succeed on the host.
pub(crate) fn script_failed(what: &str, output: &CommandOutput) -> ClientError {
    let reason = String::from_utf8_lossy(&output.stderr);
    let status = output.exit_status.map_or_else(
        || "a signal".to_owned(),
        |status| format!("status {status}"),
    );
    ClientError::Failed(format!(
        "{what} failed on the host with {status}: {}",
        reason.trim()
    ))
}
