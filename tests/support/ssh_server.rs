//! An OpenSSH server (`sshd`) that a test starts as the user who runs the
//! tests: a port above 1023 on this computer, a host key of its own, and
//! one key that can log in.

use std::fmt::Write as _;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

use crate::{ProcessGuard, wait_for};

const LOCALHOST: &str = "127.0.0.1";
/// `sshd` is not in the `PATH` of a user on each computer.
const SSHD_DIRECTORIES: [&str; 3] = ["/usr/sbin", "/usr/bin", "/usr/local/sbin"];

/// The path of `sshd`. `None` if this computer does not have it.
#[must_use]
pub fn sshd_path() -> Option<PathBuf> {
    SSHD_DIRECTORIES
        .iter()
        .map(|directory| Path::new(directory).join("sshd"))
        .find(|path| path.is_file())
}

/// Makes a new Ed25519 key pair without a passphrase at `path` and
/// `path.pub`.
///
/// # Panics
///
/// Panics if `ssh-keygen` fails.
pub fn generate_key(path: &Path) {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "nomux-test", "-f"])
        .arg(path)
        .stdin(Stdio::null())
        .status()
        .expect("ssh-keygen starts");
    assert!(
        status.success(),
        "ssh-keygen made no key at {}",
        path.display()
    );
}

/// The part of a public key file that a file of known hosts has: the key
/// type and the key in base64.
///
/// # Panics
///
/// Panics if the file is not a public key.
#[must_use]
pub fn public_key(path: &Path) -> String {
    let text = fs::read_to_string(path).expect("the public key can be read");
    let mut fields = text.split_whitespace();
    let key_type = fields.next().expect("the file has a key type");
    let key = fields.next().expect("the file has a key");
    format!("{key_type} {key}")
}

/// A running `sshd`. The server and the processes of its connections are
/// stopped when the value is dropped.
#[derive(Debug)]
pub struct SshServer {
    _process: ProcessGuard,
    port: u16,
    directory: TempDir,
}

impl SshServer {
    /// Starts `sshd`. The key `authorized_key` (a `.pub` file) can log in
    /// as the user who runs the test, and each command of that user gets
    /// `home` as its home directory. `options` are more lines of the
    /// configuration.
    ///
    /// # Panics
    ///
    /// Panics if `sshd` does not start.
    #[must_use]
    pub fn start(sshd: &Path, home: &Path, authorized_key: &Path, options: &[&str]) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("nomux-sshd")
            .tempdir_in("/tmp")
            .expect("a temporary directory can be made");
        let host_key = directory.path().join("host_key");
        generate_key(&host_key);
        let authorized_keys = directory.path().join("authorized_keys");
        fs::copy(authorized_key, &authorized_keys).expect("the key can be copied");

        let port = free_port();
        let mut configuration = format!(
            "Port {port}\n\
             ListenAddress {LOCALHOST}\n\
             HostKey {}\n\
             PidFile none\n\
             AuthorizedKeysFile {}\n\
             StrictModes no\n\
             UsePAM no\n\
             PubkeyAuthentication yes\n\
             PasswordAuthentication no\n\
             KbdInteractiveAuthentication no\n\
             SetEnv HOME={}\n",
            host_key.display(),
            authorized_keys.display(),
            home.display(),
        );
        for option in options {
            // A write to a `String` does not fail.
            let _ = writeln!(configuration, "{option}");
        }
        let configuration_file = directory.path().join("sshd_config");
        fs::write(&configuration_file, configuration).expect("the configuration can be written");

        let log = fs::File::create(directory.path().join("sshd.log")).expect("a log file");
        let process = ProcessGuard::spawn(
            Command::new(sshd)
                .arg("-D")
                .arg("-e")
                .arg("-f")
                .arg(&configuration_file)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let server = Self {
            _process: process,
            port,
            directory,
        };
        wait_for("sshd accepts connections", || {
            TcpStream::connect((LOCALHOST, port)).ok()
        });
        server
    }

    /// The port on which the server listens.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The public host key: the key type and the key in base64.
    #[must_use]
    pub fn host_key(&self) -> String {
        public_key(&self.directory.path().join("host_key.pub"))
    }

    /// What the server wrote to its log.
    #[must_use]
    pub fn log(&self) -> String {
        fs::read_to_string(self.directory.path().join("sshd.log")).unwrap_or_default()
    }
}

/// A port that no process uses now.
fn free_port() -> u16 {
    TcpListener::bind((LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("this computer has a free port")
        .port()
}
