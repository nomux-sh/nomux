//! The files of known hosts, with the trust rules of OpenSSH.
//!
//! A host is trusted only if a file has the key that the host sent. A host
//! that a file knows with a different key of the same type has a changed
//! key, and a key on a `@revoked` line is never trusted.

mod entry;

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use self::entry::{Entry, Marker};

const DEFAULT_PORT: u16 = 22;
const OWNER_ONLY_DIRECTORY: u32 = 0o700;
const OWNER_READ_WRITE: u32 = 0o600;

/// The name of a host in a file of known hosts: the host name, or
/// `[host]:port` if the port is not 22.
#[must_use]
pub fn host_label(host: &str, port: u16) -> String {
    if port == DEFAULT_PORT {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
}

/// What the files of known hosts say about the key of a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyVerdict {
    /// A file has this key for the host.
    Trusted,
    /// No file has a key of this type for the host.
    Unknown,
    /// A file has a different key of the same type for the host.
    Changed {
        /// The file.
        file: PathBuf,
        /// The line of the file.
        line: usize,
    },
    /// A file marks this key as revoked.
    Revoked {
        /// The file.
        file: PathBuf,
        /// The line of the file.
        line: usize,
    },
}

/// The lines of the files of known hosts.
#[derive(Debug, Default)]
pub struct KnownHosts {
    entries: Vec<(PathBuf, Entry)>,
}

impl KnownHosts {
    /// Reads `files`. A file that does not exist has no lines, and a line
    /// that is not valid is ignored, as OpenSSH does.
    ///
    /// # Errors
    ///
    /// Returns an error if a file exists and cannot be read.
    pub fn load(files: &[PathBuf]) -> io::Result<Self> {
        let mut entries = Vec::new();
        for file in files {
            let text = match fs::read_to_string(file) {
                Ok(text) => text,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let lines = text.lines().enumerate();
            entries.extend(
                lines
                    .filter_map(|(index, line)| Entry::parse(line, index + 1))
                    .map(|entry| (file.clone(), entry)),
            );
        }
        Ok(Self { entries })
    }

    /// The key types that the files have for `host`. The client asks the
    /// host for a key of one of these types first, as OpenSSH does. Thus a
    /// host that is known cannot get a new trust decision with a key of a
    /// different type.
    #[must_use]
    pub fn key_types(&self, host: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.marker == Marker::None && entry.names(host))
            .map(|(_, entry)| entry.key_type.as_str())
            .collect()
    }

    /// Checks the key that `host` sent: its type and its encoded form.
    #[must_use]
    pub fn check(&self, host: &str, key_type: &str, key: &[u8]) -> HostKeyVerdict {
        let mut trusted = false;
        let mut changed = None;
        for (file, entry) in &self.entries {
            if !entry.names(host) {
                continue;
            }
            let same_key = entry.key_type == key_type && entry.key == key;
            match entry.marker {
                Marker::Revoked if same_key => {
                    return HostKeyVerdict::Revoked {
                        file: file.clone(),
                        line: entry.line,
                    };
                }
                Marker::None if same_key => trusted = true,
                Marker::None if entry.key_type == key_type && changed.is_none() => {
                    changed = Some(HostKeyVerdict::Changed {
                        file: file.clone(),
                        line: entry.line,
                    });
                }
                Marker::None | Marker::Revoked | Marker::CertAuthority => {}
            }
        }
        match changed {
            _ if trusted => HostKeyVerdict::Trusted,
            Some(changed) => changed,
            None => HostKeyVerdict::Unknown,
        }
    }
}

/// Adds the key of `host` to `file`: one line with the host name, the key
/// type and the key in base64. Makes the file, with access only for the
/// user, if it does not exist.
///
/// # Errors
///
/// Returns an error if the directory or the file cannot be written.
pub fn add_host_key(file: &Path, host: &str, key_type: &str, key_base64: &str) -> io::Result<()> {
    if let Some(directory) = file.parent() {
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY_DIRECTORY)
            .create(directory)?;
    }
    let ends_with_newline =
        fs::read(file).map_or(true, |text| text.last().is_none_or(|&last| last == b'\n'));
    let mut known_hosts = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(OWNER_READ_WRITE)
        .open(file)?;
    let separator = if ends_with_newline { "" } else { "\n" };
    writeln!(known_hosts, "{separator}{host} {key_type} {key_base64}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED25519_A: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const ED25519_B: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G1sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X";

    fn decode(key: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(key)
            .unwrap()
    }

    fn known_hosts(text: &str) -> (tempfile::TempDir, KnownHosts) {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("known_hosts");
        fs::write(&file, text).unwrap();
        let known_hosts = KnownHosts::load(&[file, directory.path().join("absent")]).unwrap();
        (directory, known_hosts)
    }

    #[test]
    fn the_same_key_is_trusted_and_a_different_key_of_the_type_is_a_change() {
        let (directory, hosts) = known_hosts(&format!(
            "# comment\n\nvps.example.com,10.0.0.7 ssh-ed25519 {ED25519_A} ada@laptop\n"
        ));

        assert_eq!(
            hosts.check("vps.example.com", "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Trusted
        );
        assert_eq!(
            hosts.check("10.0.0.7", "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Trusted
        );
        assert_eq!(
            hosts.check("vps.example.com", "ssh-ed25519", &decode(ED25519_B)),
            HostKeyVerdict::Changed {
                file: directory.path().join("known_hosts"),
                line: 3,
            }
        );
        assert_eq!(
            hosts.check("other.example.com", "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Unknown
        );
    }

    #[test]
    fn a_key_of_a_type_that_the_file_does_not_have_is_unknown_and_the_known_types_are_given() {
        let (_directory, hosts) =
            known_hosts(&format!("vps.example.com ssh-ed25519 {ED25519_A}\n"));

        assert_eq!(
            hosts.check("vps.example.com", "ssh-rsa", b"an RSA key"),
            HostKeyVerdict::Unknown
        );
        assert_eq!(hosts.key_types("vps.example.com"), ["ssh-ed25519"]);
        assert_eq!(hosts.key_types("other.example.com"), [""; 0]);
    }

    #[test]
    fn a_port_that_is_not_22_is_part_of_the_name() {
        let (_directory, hosts) =
            known_hosts(&format!("[vps.example.com]:2222 ssh-ed25519 {ED25519_A}\n"));

        assert_eq!(host_label("vps.example.com", 22), "vps.example.com");
        let label = host_label("vps.example.com", 2222);
        assert_eq!(label, "[vps.example.com]:2222");
        assert_eq!(
            hosts.check(&label, "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Trusted
        );
        assert_eq!(
            hosts.check("vps.example.com", "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Unknown
        );
    }

    #[test]
    fn a_revoked_key_is_never_trusted() {
        let (directory, hosts) = known_hosts(&format!(
            "vps.example.com ssh-ed25519 {ED25519_A}\n@revoked * ssh-ed25519 {ED25519_A}\n"
        ));

        assert_eq!(
            hosts.check("vps.example.com", "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Revoked {
                file: directory.path().join("known_hosts"),
                line: 2,
            }
        );
    }

    #[test]
    fn a_new_key_is_added_on_its_own_line_and_is_trusted_then() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join(".ssh").join("known_hosts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "old.example.com ssh-ed25519 AAAA").unwrap();

        add_host_key(&file, "[vps.example.com]:2222", "ssh-ed25519", ED25519_A).unwrap();

        let hosts = KnownHosts::load(std::slice::from_ref(&file)).unwrap();
        assert_eq!(
            hosts.check("[vps.example.com]:2222", "ssh-ed25519", &decode(ED25519_A)),
            HostKeyVerdict::Trusted
        );
        assert_eq!(fs::read_to_string(&file).unwrap().lines().count(), 2);
    }
}
