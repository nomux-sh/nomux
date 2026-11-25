//! The three nomux binaries that the client installs on a remote host, and
//! where the client gets them.

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::VERSION;
use crate::ClientError;

/// The binaries of an installation.
pub(crate) const BINARY_NAMES: [&str; 3] = ["nomux", "nomuxd", "nomux-holder"];

const DEFAULT_DOWNLOAD_URL: &str = "https://github.com/nomux-sh/nomux/releases/download";
/// Replaces the address of the releases, as for `install.sh`.
const DOWNLOAD_URL_VARIABLE: &str = "NOMUX_RELEASE_DOWNLOAD_URL";
/// Names a local directory with the binaries for the remote host.
const DIRECTORY_VARIABLE: &str = "NOMUX_BOOTSTRAP_BINARIES";

/// Where the client gets the binaries for a remote host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinarySource {
    /// The release archive of the version of this client. The value is the
    /// address of the releases: the archive of the version `V` and the
    /// target `T` is at `<address>/vV/nomux-V-T.tar.gz`.
    Release(String),
    /// A local directory that has `nomux`, `nomuxd` and `nomux-holder` for
    /// the operating system and the processor of the remote host. A build
    /// that has no release uses it.
    Directory(PathBuf),
}

impl BinarySource {
    /// The source that the environment of the process selects: the
    /// directory of the `NOMUX_BOOTSTRAP_BINARIES` variable, or the
    /// releases at the address of the `NOMUX_RELEASE_DOWNLOAD_URL`
    /// variable, or the releases on GitHub.
    #[must_use]
    pub fn from_environment() -> Self {
        let variable = |name: &str| env::var_os(name).filter(|value| !value.is_empty());
        if let Some(directory) = variable(DIRECTORY_VARIABLE) {
            return Self::Directory(PathBuf::from(directory));
        }
        let url = variable(DOWNLOAD_URL_VARIABLE)
            .and_then(|url| url.into_string().ok())
            .unwrap_or_else(|| DEFAULT_DOWNLOAD_URL.to_owned());
        Self::Release(url)
    }

    /// The name of the installation directory in `~/.nomux/bin` on a host
    /// for the binaries of this source.
    pub(crate) fn label(&self) -> Result<String, ClientError> {
        match self {
            Self::Release(_) => Ok(VERSION.to_owned()),
            Self::Directory(directory) => Ok(read_directory(directory)?.label()),
        }
    }
}

/// Reads the binaries of the directory of a [`BinarySource::Directory`].
pub(crate) fn read_directory(directory: &Path) -> Result<Binaries, ClientError> {
    Binaries::read(directory).map_err(|error| {
        ClientError::Failed(format!(
            "the directory {} of the {DIRECTORY_VARIABLE} variable must have the files nomux, \
             nomuxd and nomux-holder: {error}",
            directory.display()
        ))
    })
}

/// How a user gets the binaries of a build that has no release.
pub(crate) fn directory_hint(target: &str) -> String {
    format!(
        "A development build has no release: build nomux, nomuxd and nomux-holder for {target} \
         and set the {DIRECTORY_VARIABLE} variable to the directory that has them"
    )
}

/// The content of the three binaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binaries {
    /// The name and the bytes of each binary, in the sequence of
    /// [`BINARY_NAMES`].
    files: Vec<(&'static str, Vec<u8>)>,
}

impl Binaries {
    /// Reads the three binaries from `directory`.
    pub(crate) fn read(directory: &Path) -> std::io::Result<Self> {
        let files = BINARY_NAMES
            .iter()
            .map(|&name| Ok((name, fs::read(directory.join(name))?)))
            .collect::<std::io::Result<_>>()?;
        Ok(Self { files })
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = (&'static str, &[u8])> {
        self.files
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
    }

    pub(crate) fn total_size(&self) -> usize {
        self.files.iter().map(|(_, bytes)| bytes.len()).sum()
    }

    /// The name of the installation directory for binaries that are not
    /// from a release: the version and the first 12 characters of the
    /// SHA-256 checksum of the three checksums. Thus two builds of one
    /// version get two directories.
    pub(crate) fn label(&self) -> String {
        let checksums: String = self.files().map(|(_, bytes)| sha256_hex(bytes)).collect();
        let checksum = sha256_hex(checksums.as_bytes());
        format!("{VERSION}-{}", &checksum[..12])
    }
}

/// The SHA-256 checksum of `bytes` in lower-case hexadecimal, as
/// `sha256sum` prints it.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            // A write to a `String` does not fail.
            let _ = write!(text, "{byte:02x}");
            text
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_is_the_text_that_sha256sum_prints() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_changed_binary_gives_a_different_installation_directory() {
        let directory = tempfile::tempdir().unwrap();
        for name in BINARY_NAMES {
            fs::write(directory.path().join(name), name).unwrap();
        }
        let first = Binaries::read(directory.path()).unwrap().label();
        fs::write(directory.path().join("nomux-holder"), "a new build").unwrap();
        let second = Binaries::read(directory.path()).unwrap().label();

        assert_eq!(first.len(), VERSION.len() + 1 + 12);
        assert!(first.starts_with(VERSION));
        assert_ne!(first, second);
    }

    #[test]
    fn a_directory_without_a_binary_is_an_error_that_names_the_variable() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("nomux"), "only one").unwrap();

        let message = read_directory(directory.path()).unwrap_err().to_string();

        assert!(message.contains("NOMUX_BOOTSTRAP_BINARIES"), "{message}");
    }
}
