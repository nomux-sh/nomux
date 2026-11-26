//! The release archive of an extension for `nomux ext install`: a `.tar.gz`
//! file on this computer, an `https` address of one, or the release of an
//! extension of the organization for its short form (`git`, `git@0.1.0`).
//!
//! The client gets the archive and its checksum file, compares the SHA-256
//! checksum before it opens the archive, reads the entries in memory and
//! checks them, and refuses an archive for a different platform than the
//! host. The caller shows the manifest to the user. Only after that, the
//! host gets the files of the package: [`Host::stage_extension_package`].
//!
//! - `archive_entries`: the entries of the archive and their rules.
//! - `release_address`: the addresses of the archive and of its checksum.
//! - `https_download`: the download with `curl`.
//!
//! [`Host::stage_extension_package`]: crate::Host::stage_extension_package

mod archive_entries;
mod https_download;
mod release_address;

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use archive_entries::{ArchiveEntries, EntryKind, PackageEntry};
use https_download::MAX_DOWNLOAD_BYTES;
use release_address::{CHECKSUM_FILE, RELEASE_LIST_URL, ShortForm};
use tempfile::TempDir;

use crate::ClientError;
use crate::bootstrap::{RELEASE_TARGETS, check_sha256};

const MANIFEST_FILE: &str = "extension.toml";
const OWNER_ONLY: u32 = 0o700;
const OWNER_READ_WRITE: u32 = 0o600;
const OWNER_EXECUTE: u32 = 0o700;

/// Where `nomux ext install` gets a package that is not a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveSource {
    /// A release archive on this computer.
    File(PathBuf),
    /// The address of a release archive.
    Url(String),
    /// The release of an extension of the organization: its name and, if
    /// the user gave one, its version.
    Release {
        /// The name of the directory of the extension in the repository
        /// `nomux-sh/extensions`, for example `git`.
        name: String,
        /// The version `X.Y.Z`. `None`: the newest release.
        version: Option<String>,
    },
}

impl ArchiveSource {
    /// The source that `text` names. `None` for a directory, and for a path
    /// that does not exist and is not a short form: such a text is the
    /// path of a package directory.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.contains("://") {
            return Some(Self::Url(text.to_owned()));
        }
        let path = Path::new(text);
        if path.is_dir() {
            return None;
        }
        if path.is_file() {
            return Some(Self::File(path.to_owned()));
        }
        ShortForm::parse(text).map(|short| Self::Release {
            name: short.name,
            version: short.version,
        })
    }
}

/// How the client compared the checksum of an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumCheck {
    /// The `SHA256SUMS` file of the release has the checksum of the
    /// archive.
    Verified,
    /// A file on this computer without a `SHA256SUMS` file next to it.
    NoChecksumFile,
}

/// A release archive whose entries the client checked.
#[derive(Debug)]
pub struct ExtensionArchive {
    /// The path or the address of the archive.
    pub source: String,
    /// The name of the package directory: `nomux-<name>-<version>-<target>`.
    pub package: String,
    /// The release target of the archive.
    pub target: String,
    /// How the client compared the checksum.
    pub checksum: ChecksumCheck,
    /// The text of `extension.toml`.
    pub manifest: String,
    entries: Vec<PackageEntry>,
}

impl ExtensionArchive {
    /// The directories and the files of the package, with their paths in
    /// the package directory: `true` for an executable file.
    pub(crate) fn files(&self) -> impl Iterator<Item = (&Path, Option<(&[u8], bool)>)> {
        self.entries.iter().map(|entry| match &entry.kind {
            EntryKind::Directory => (entry.path.as_path(), None),
            EntryKind::File {
                executable,
                content,
            } => (
                entry.path.as_path(),
                Some((content.as_slice(), *executable)),
            ),
        })
    }

    /// Writes the package into a new directory below `parent` and gives
    /// the path of the package directory.
    pub(crate) fn write_package(&self, parent: &Path) -> io::Result<PathBuf> {
        let package = parent.join(&self.package);
        create_private_directory(&package)?;
        for (path, file) in self.files() {
            let path = package.join(path);
            match file {
                None => create_private_directory(&path)?,
                Some((content, executable)) => {
                    let mode = if executable {
                        OWNER_EXECUTE
                    } else {
                        OWNER_READ_WRITE
                    };
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(mode)
                        .open(&path)?
                        .write_all(content)?;
                }
            }
        }
        Ok(package)
    }
}

/// Gets the archive of `source` for the release target `target` of the
/// host, compares its checksum and checks its entries. Writes only into a
/// temporary directory of the user, which it removes.
///
/// # Errors
///
/// Returns [`ClientError::Failed`] for a download that fails, a wrong
/// checksum, an archive that the rules refuse, and an archive for a
/// different target.
pub async fn fetch_extension_archive(
    source: &ArchiveSource,
    target: &str,
) -> Result<ExtensionArchive, ClientError> {
    let work = WorkDirectory::new()?;
    let (source, archive, checksum) = match source {
        ArchiveSource::File(path) => {
            let archive = read_limited(path)?;
            let checksums = path.with_file_name(CHECKSUM_FILE);
            let checksum = if checksums.is_file() {
                check_sha256(&archive, &file_name_of(path), &read_limited(&checksums)?)?;
                ChecksumCheck::Verified
            } else {
                ChecksumCheck::NoChecksumFile
            };
            (path.display().to_string(), archive, checksum)
        }
        ArchiveSource::Url(url) => {
            release_address::check_url(url).map_err(ClientError::Failed)?;
            let checksum_url = release_address::checksum_url(url);
            let archive = work
                .download(url, release_address::file_name(url), &checksum_url)
                .await?;
            (url.clone(), archive, ChecksumCheck::Verified)
        }
        ArchiveSource::Release { name, version } => {
            let version = match version {
                Some(version) => version.clone(),
                None => work.newest_version(name).await?,
            };
            let (url, checksum_url) = release_address::release_urls(name, &version, target);
            let archive = work
                .download(&url, release_address::file_name(&url), &checksum_url)
                .await?;
            (url, archive, ChecksumCheck::Verified)
        }
    };
    checked_archive(source, &archive, checksum, target)
}

/// The archive `bytes` after the checks of its entries and of its target.
fn checked_archive(
    source: String,
    bytes: &[u8],
    checksum: ChecksumCheck,
    target: &str,
) -> Result<ExtensionArchive, ClientError> {
    let refused = |reason: String| {
        ClientError::Failed(format!(
            "the archive {source} is refused: {reason}. Nothing was installed"
        ))
    };
    let ArchiveEntries { package, entries } = archive_entries::read(bytes).map_err(refused)?;
    let archive_target = RELEASE_TARGETS
        .iter()
        .find(|known| package.ends_with(&format!("-{known}")))
        .ok_or_else(|| {
            refused(format!(
                "its package directory {package} does not end with a release target ({})",
                RELEASE_TARGETS.join(", ")
            ))
        })?;
    if *archive_target != target {
        return Err(refused(format!(
            "it is for {archive_target}, and the host is {target}"
        )));
    }
    let manifest = entries
        .iter()
        .find_map(|entry| match &entry.kind {
            EntryKind::File { content, .. } if entry.path == Path::new(MANIFEST_FILE) => {
                Some(content)
            }
            _ => None,
        })
        .ok_or_else(|| refused(format!("it has no {MANIFEST_FILE} in {package}")))?;
    let manifest = String::from_utf8(manifest.clone())
        .map_err(|_| refused(format!("its {MANIFEST_FILE} is not UTF-8")))?;
    Ok(ExtensionArchive {
        source,
        package,
        target: (*archive_target).to_owned(),
        checksum,
        manifest,
        entries,
    })
}

/// The directory of the downloads of this user: `~/.nomux/downloads`. Not
/// `/tmp`: each user can write there.
pub(crate) fn downloads_directory() -> Result<PathBuf, ClientError> {
    let runtime = nomux_core::runtime_directory::locate()
        .map_err(|error| ClientError::Failed(error.to_string()))?;
    let directory = runtime
        .parent()
        .map_or_else(|| runtime.clone(), Path::to_owned)
        .join("downloads");
    create_private_directory_all(&directory).map_err(|error| local_failure(&directory, &error))?;
    Ok(directory)
}

/// A temporary directory of the user for one download. It goes when the
/// value is dropped.
struct WorkDirectory(TempDir);

impl WorkDirectory {
    fn new() -> Result<Self, ClientError> {
        let parent = downloads_directory()?;
        tempfile::Builder::new()
            .prefix("download-")
            .tempdir_in(&parent)
            .map(Self)
            .map_err(|error| local_failure(&parent, &error))
    }

    /// A new empty file `name` of the directory, readable only by the user.
    fn new_file(&self, name: &str) -> Result<PathBuf, ClientError> {
        let path = self.0.path().join(name);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(OWNER_READ_WRITE)
            .open(&path)
            .map_err(|error| local_failure(&path, &error))?;
        Ok(path)
    }

    /// Downloads the archive `url` and its checksum file, compares the
    /// checksum and gives the bytes of the archive.
    async fn download(
        &self,
        url: &str,
        archive_name: &str,
        checksum_url: &str,
    ) -> Result<Vec<u8>, ClientError> {
        let archive = self.new_file(archive_name)?;
        let checksums = self.new_file(CHECKSUM_FILE)?;
        https_download::download(url, &archive).await?;
        https_download::download(checksum_url, &checksums).await?;
        let archive = read_limited(&archive)?;
        check_sha256(&archive, archive_name, &read_limited(&checksums)?)?;
        Ok(archive)
    }

    async fn newest_version(&self, name: &str) -> Result<String, ClientError> {
        let list = self.new_file("releases.json")?;
        https_download::download(RELEASE_LIST_URL, &list).await?;
        release_address::newest_version(name, &read_limited(&list)?).map_err(ClientError::Failed)
    }
}

/// The bytes of the file at `path`, at most [`MAX_DOWNLOAD_BYTES`].
fn read_limited(path: &Path) -> Result<Vec<u8>, ClientError> {
    let file = File::open(path).map_err(|error| local_failure(path, &error))?;
    let mut bytes = Vec::new();
    file.take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| local_failure(path, &error))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(ClientError::Failed(format!(
            "the archive {} has more than {} MiB",
            path.display(),
            MAX_DOWNLOAD_BYTES / 1024 / 1024
        )));
    }
    Ok(bytes)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(OWNER_ONLY).create(path)
}

fn create_private_directory_all(path: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(OWNER_ONLY)
        .create(path)?;
    fs::set_permissions(
        path,
        std::os::unix::fs::PermissionsExt::from_mode(OWNER_ONLY),
    )
}

fn local_failure(path: &Path, error: &io::Error) -> ClientError {
    ClientError::Failed(format!("{}: {error}", path.display()))
}
