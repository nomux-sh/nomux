//! The release archive of the version of this client: download, checksum
//! and content.
//!
//! The client uses `curl` and `tar`, the programs that `install.sh` also
//! needs. Thus a computer that installed nomux with the script has them.

use std::io;
use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

use super::binaries::{Binaries, directory_hint, sha256_hex};
use super::{BootstrapStep, VERSION};
use crate::ClientError;

const CHECKSUM_FILE: &str = "SHA256SUMS";
/// The exit status of `curl --fail` for an HTTP status of 400 or more.
const CURL_HTTP_ERROR: i32 = 22;
/// The exit status of `curl` for a `file://` address without a file.
const CURL_FILE_NOT_READ: i32 = 37;

/// Downloads the archive of `target` from the releases at `releases_url`, verifies its SHA-256 checksum against
/// the `SHA256SUMS` file of the release and reads the three binaries.
pub(crate) async fn download(
    releases_url: &str,
    target: &str,
    progress: &(dyn Fn(BootstrapStep<'_>) + Send + Sync),
) -> Result<Binaries, ClientError> {
    let name = format!("nomux-{VERSION}-{target}");
    let archive_name = format!("{name}.tar.gz");
    let version_url = format!("{}/v{VERSION}", releases_url.trim_end_matches('/'));
    let archive_url = format!("{version_url}/{archive_name}");
    let work =
        tempfile::tempdir().map_err(|error| local_failure("a temporary directory", &error))?;
    let archive = work.path().join(&archive_name);
    let checksums = work.path().join(CHECKSUM_FILE);

    progress(BootstrapStep::Downloading { url: &archive_url });
    fetch(&archive_url, &archive, target).await?;
    fetch(
        &format!("{version_url}/{CHECKSUM_FILE}"),
        &checksums,
        target,
    )
    .await?;
    verify_checksum(&archive, &archive_name, &checksums)?;

    let extracted = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(work.path())
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| missing_program("tar", &error, target))?;
    if !extracted.status.success() {
        return Err(ClientError::Failed(format!(
            "the archive {archive_url} cannot be extracted: {}",
            String::from_utf8_lossy(&extracted.stderr).trim()
        )));
    }
    Binaries::read(&work.path().join(&name)).map_err(|error| {
        ClientError::Failed(format!(
            "the archive {archive_url} does not have the three binaries: {error}"
        ))
    })
}

async fn fetch(url: &str, file: &Path, target: &str) -> Result<(), ClientError> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(file)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| missing_program("curl", &error, target))?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(CURL_HTTP_ERROR | CURL_FILE_NOT_READ) => Err(ClientError::Failed(format!(
            "nomux {VERSION} has no release archive for {target} at {url}. {}",
            directory_hint(target)
        ))),
        _ => Err(ClientError::Failed(format!(
            "the download of {url} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

/// Compares the checksum of the archive with its line in `SHA256SUMS`:
/// `<checksum>  <file name>`.
fn verify_checksum(
    archive: &Path,
    archive_name: &str,
    checksums: &Path,
) -> Result<(), ClientError> {
    let read = |file: &Path| {
        std::fs::read(file).map_err(|error| local_failure(&file.display().to_string(), &error))
    };
    let checksums = String::from_utf8_lossy(&read(checksums)?).into_owned();
    let expected = checksums
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, name)| name.trim_start_matches([' ', '*']) == archive_name)
        .map(|(checksum, _)| checksum.to_ascii_lowercase())
        .ok_or_else(|| {
            ClientError::Failed(format!(
                "{CHECKSUM_FILE} has no checksum for {archive_name}"
            ))
        })?;
    let actual = sha256_hex(&read(archive)?);
    if actual == expected {
        Ok(())
    } else {
        Err(ClientError::Failed(format!(
            "the checksum of {archive_name} is wrong: {CHECKSUM_FILE} has {expected}, the \
             download has {actual}. Nothing was installed"
        )))
    }
}

fn missing_program(program: &str, error: &io::Error, target: &str) -> ClientError {
    ClientError::Failed(format!(
        "the command `{program}` is necessary to get the release archive: {error}. Install \
         `{program}`, or use binaries from a directory. {}",
        directory_hint(target)
    ))
}

fn local_failure(what: &str, error: &io::Error) -> ClientError {
    ClientError::Failed(format!("{what} cannot be used: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn an_archive_with_a_different_checksum_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("nomux-1.0.0-t.tar.gz");
        let checksums = directory.path().join(CHECKSUM_FILE);
        fs::write(&archive, "abc").unwrap();
        let correct = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        fs::write(&checksums, format!("{correct}  nomux-1.0.0-t.tar.gz\n")).unwrap();
        assert!(verify_checksum(&archive, "nomux-1.0.0-t.tar.gz", &checksums).is_ok());

        fs::write(&archive, "abd").unwrap();
        let message = verify_checksum(&archive, "nomux-1.0.0-t.tar.gz", &checksums)
            .unwrap_err()
            .to_string();
        assert!(message.contains("checksum"), "{message}");

        fs::write(&checksums, format!("{correct}  a-different-file.tar.gz\n")).unwrap();
        assert!(verify_checksum(&archive, "nomux-1.0.0-t.tar.gz", &checksums).is_err());
    }
}
