//! A remote host for a test: an SSH server on this computer whose user has
//! a home directory of its own, and a client with its own home directory,
//! key and file of known hosts.
//!
//! The client reaches the server through a relay, thus a test can cut the
//! connection.

use std::fs::{self, DirBuilder, Permissions};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ssh_server::{SshServer, generate_key, sshd_path};
use crate::tcp_relay::{Impairment, TcpRelay};
use crate::{CliRun, Sandbox, TerminalClient, binary};

const BINARIES: [&str; 3] = ["nomux", "nomuxd", "nomux-holder"];
/// The version of the binaries that the tests run.
const VERSION: &str = env!("CARGO_PKG_VERSION");
const OWNER_ONLY: u32 = 0o700;

/// A client and a remote host.
///
/// The fields are in the sequence in which they must end: the client, then
/// the daemon and the holders of the host, then the relay and the server.
#[derive(Debug)]
pub struct RemoteHost {
    /// The side of the user: `~/.ssh` and the configuration of nomux.
    pub client: Sandbox,
    /// The home directory of the user on the host.
    pub host: Sandbox,
    relay: TcpRelay,
    server: SshServer,
    user: String,
}

impl RemoteHost {
    /// Starts a host that has the nomux binaries of this build in
    /// `~/.nomux/bin/<version>`, as after `nomux host add`. The client
    /// knows the host key.
    ///
    /// Returns `None`, with the reason on the standard error, if this
    /// computer has no `sshd`: the test cannot run then.
    #[must_use]
    pub fn start() -> Option<Self> {
        let remote = Self::start_with(&[], Impairment::default())?;
        remote.install_binaries();
        Some(remote)
    }

    /// Starts a host without the nomux binaries. `sshd_options` are more
    /// lines of the configuration of the server, and `impairment` is what
    /// the connection does to the bytes.
    ///
    /// # Panics
    ///
    /// Panics if the server or a key cannot be made.
    #[must_use]
    pub fn start_with(sshd_options: &[&str], impairment: Impairment) -> Option<Self> {
        let Some(sshd) = sshd_path() else {
            eprintln!("SKIPPED: this computer has no sshd, thus no remote host for the test");
            return None;
        };
        let mut client = Sandbox::new();
        let host = Sandbox::new();
        let ssh_directory = client.home().join(".ssh");
        DirBuilder::new()
            .mode(OWNER_ONLY)
            .create(&ssh_directory)
            .expect("~/.ssh can be made");
        let key = ssh_directory.join("id_ed25519");
        generate_key(&key);

        let server = SshServer::start(&sshd, host.home(), &key.with_extension("pub"), sshd_options);
        let relay = TcpRelay::start(server.port(), impairment);
        let user = current_user();
        client.set_variable("USER", &user);
        let remote = Self {
            client,
            host,
            relay,
            server,
            user,
        };
        remote.write_known_hosts(&remote.server.host_key());
        Some(remote)
    }

    /// The SSH destination of the host, through the relay.
    #[must_use]
    pub fn ssh_target(&self) -> String {
        format!("ssh://{}@127.0.0.1:{}", self.user, self.relay.port())
    }

    /// The SSH destination of the server without the relay.
    #[must_use]
    pub fn direct_ssh_target(&self) -> String {
        format!("ssh://{}@127.0.0.1:{}", self.user, self.server.port())
    }

    /// The name of the host in the file of known hosts of the client.
    #[must_use]
    pub fn known_hosts_name(&self) -> String {
        format!("[127.0.0.1]:{}", self.relay.port())
    }

    /// The file of known hosts of the client.
    #[must_use]
    pub fn known_hosts_file(&self) -> PathBuf {
        self.client.home().join(".ssh").join("known_hosts")
    }

    /// Makes `key` (the key type and the key in base64) the only key that
    /// the client knows for the host, through the relay and without it.
    ///
    /// # Panics
    ///
    /// Panics if the file cannot be written.
    pub fn write_known_hosts(&self, key: &str) {
        let lines = format!(
            "{} {key}\n[127.0.0.1]:{} {key}\n",
            self.known_hosts_name(),
            self.server.port()
        );
        fs::write(self.known_hosts_file(), lines).expect("known_hosts can be written");
    }

    /// Puts the binaries of this build into `~/.nomux/bin/<version>` of the
    /// host, as links.
    ///
    /// # Panics
    ///
    /// Panics if a directory or a link cannot be made.
    pub fn install_binaries(&self) {
        let directory = self.installation_directory();
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(&directory)
            .expect("the installation directory can be made");
        for name in BINARIES {
            symlink(binary(name), directory.join(name)).expect("a link can be made");
        }
    }

    /// `~/.nomux/bin/<version>` of the host.
    #[must_use]
    pub fn installation_directory(&self) -> PathBuf {
        self.host.home().join(".nomux").join("bin").join(VERSION)
    }

    /// Makes a directory with the release archive of this build for this
    /// computer and its `SHA256SUMS` file, and returns the address that
    /// `NOMUX_RELEASE_DOWNLOAD_URL` takes.
    ///
    /// # Panics
    ///
    /// Panics if `tar` or the checksum program fails.
    #[must_use]
    pub fn release_fixture(&self) -> String {
        let releases = self.client.home().join("releases");
        let release = releases.join(format!("v{VERSION}"));
        let name = format!("nomux-{VERSION}-{}", release_target());
        let content = release.join(&name);
        fs::create_dir_all(&content).expect("the release directory can be made");
        for binary_name in BINARIES {
            fs::copy(binary(binary_name), content.join(binary_name)).expect("a binary is copied");
        }
        let archive = format!("{name}.tar.gz");
        run(Command::new("tar")
            .args(["-czf", &archive, &name])
            .current_dir(&release));
        fs::remove_dir_all(&content).expect("the copy can be removed");
        let checksum_program = if Path::new("/usr/bin/sha256sum").is_file() {
            "sha256sum"
        } else {
            "shasum -a 256"
        };
        run(Command::new("sh")
            .arg("-c")
            .arg(format!("{checksum_program} {archive} > SHA256SUMS"))
            .current_dir(&release));
        format!("file://{}", releases.display())
    }

    /// Runs `nomux` as the user of the client.
    #[must_use]
    pub fn nomux(&self, arguments: &[&str]) -> CliRun {
        self.client.nomux(arguments)
    }

    /// Runs `nomux` as the user of the client and makes sure that it
    /// succeeded.
    ///
    /// # Panics
    ///
    /// Panics if the exit code is not 0. The message has the log of the
    /// SSH server.
    #[must_use]
    pub fn nomux_ok(&self, arguments: &[&str]) -> CliRun {
        let run = self.nomux(arguments);
        assert_eq!(
            run.code,
            0,
            "nomux {arguments:?}: {run:?}\nsshd: {}",
            self.server.log()
        );
        run
    }

    /// Starts `nomux` as the user of the client in a terminal.
    #[must_use]
    pub fn nomux_in_terminal(&self, arguments: &[&str]) -> TerminalClient {
        self.client.nomux_in_terminal(arguments)
    }

    /// Adds the host to the configuration of the client with the name
    /// `vps`, as `nomux host add` does.
    ///
    /// # Panics
    ///
    /// Panics if the command fails.
    pub fn add_as_vps(&self) {
        let _ = self.nomux_ok(&["host", "add", &self.ssh_target(), "--name", "vps"]);
    }

    /// Ends each SSH connection of the client, as a network failure does.
    pub fn cut_connections(&self) {
        self.relay.cut_connections();
    }

    /// Gives the mode `mode` to a path below the home directory of the
    /// host.
    ///
    /// # Panics
    ///
    /// Panics if the mode cannot be set.
    pub fn set_mode_on_host(&self, path: &str, mode: u32) {
        fs::set_permissions(self.host.home().join(path), Permissions::from_mode(mode))
            .expect("the mode can be set");
    }
}

/// The release target of this computer, as `install.sh` and the client
/// name it.
fn release_target() -> String {
    let processor = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => format!("{processor}-apple-darwin"),
        _ => format!("{processor}-unknown-linux-musl"),
    }
}

fn current_user() -> String {
    let output = Command::new("id").arg("-un").output().expect("id starts");
    String::from_utf8(output.stdout)
        .expect("a user name is UTF-8")
        .trim()
        .to_owned()
}

fn run(command: &mut Command) {
    let status = command.status().expect("the command starts");
    assert!(status.success(), "{command:?} failed: {status}");
}
