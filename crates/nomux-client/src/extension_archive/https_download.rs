//! The download of a file with the `curl` program of the user: only
//! `https`, also for each redirect, with a time limit and a size limit.
//!
//! nomux has no TLS client of its own: a TLS client in the `nomux` binary
//! costs 1.5 MB, and the remote bootstrap and `install.sh` need `curl`
//! already. Only `nomux ext install` downloads.

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

use crate::ClientError;

/// The largest file that a download gives: 64 MiB.
pub(crate) const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
const CONNECT_TIMEOUT_SECONDS: u32 = 15;
const MAX_TIME_SECONDS: u32 = 300;
const MAX_REDIRECTS: u32 = 10;
/// The exit status of `curl` for a protocol that `--proto` does not permit.
const CURL_UNSUPPORTED_PROTOCOL: i32 = 1;
/// The exit status of `curl --fail` for an HTTP status of 400 or more.
const CURL_HTTP_ERROR: i32 = 22;
/// The exit status of `curl` at the end of its time limit.
const CURL_TIMEOUT: i32 = 28;
/// The exit status of `curl` for a file above `--max-filesize`.
const CURL_FILE_TOO_LARGE: i32 = 63;

/// Downloads `url` into `file`, which exists and is empty. The caller
/// checked the address.
///
/// # Errors
///
/// Returns [`ClientError::Failed`] if `curl` is missing, the server answers
/// with an error, a redirect goes to an address that is not `https`, or a
/// limit is reached.
pub(crate) async fn download(url: &str, file: &Path) -> Result<(), ClientError> {
    let output = Command::new("curl")
        // First: no `.curlrc` of the user can change the options below.
        .arg("--disable")
        .args(["--proto", "=https", "--proto-redir", "=https", "--tlsv1.2"])
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--max-redirs", &MAX_REDIRECTS.to_string()])
        .args(["--connect-timeout", &CONNECT_TIMEOUT_SECONDS.to_string()])
        .args(["--max-time", &MAX_TIME_SECONDS.to_string()])
        .args(["--max-filesize", &MAX_DOWNLOAD_BYTES.to_string()])
        .arg("--output")
        .arg(file)
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| {
            ClientError::Failed(format!(
                "the program `curl` is necessary to download {url}: {error}. Install the package \
                 `curl`, or download the archive by hand and give its path to `nomux ext install`"
            ))
        })?;
    let reason = || String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let failure = match output.status.code() {
        Some(0) => return check_size(url, file),
        Some(CURL_HTTP_ERROR) => format!("the server answered with an error: {}", reason()),
        Some(CURL_UNSUPPORTED_PROTOCOL) => {
            "the server sent a redirect to an address that is not https".to_owned()
        }
        Some(CURL_TIMEOUT) => format!("no answer in {MAX_TIME_SECONDS} seconds"),
        Some(CURL_FILE_TOO_LARGE) => {
            format!(
                "the file has more than {} MiB",
                MAX_DOWNLOAD_BYTES / 1024 / 1024
            )
        }
        _ => reason(),
    };
    Err(ClientError::Failed(format!(
        "the download of {url} failed: {failure}. Nothing was installed"
    )))
}

/// `--max-filesize` does not stop a download whose size the server does not
/// say before.
fn check_size(url: &str, file: &Path) -> Result<(), ClientError> {
    let size = std::fs::metadata(file)
        .map_err(|error| ClientError::Failed(format!("{}: {error}", file.display())))?
        .len();
    if size > MAX_DOWNLOAD_BYTES {
        return Err(ClientError::Failed(format!(
            "the download of {url} failed: the file has more than {} MiB. Nothing was installed",
            MAX_DOWNLOAD_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}
