//! `ext install` of a release archive: a `.tar.gz` file, an `https`
//! address, or the short form of an extension of the organization.
//!
//! The CLI shows the name, the version and the permissions of the extension
//! before anything is written on the host, and writes only after the user
//! accepted: the answer `y` to the question, or `--yes`.

use std::io::{self, BufRead, IsTerminal, Write};

use nomux_client::{
    ArchiveSource, ChecksumCheck, Client, ClientError, ExtensionArchive, ExtensionPackage, Host,
    fetch_extension_archive,
};

use super::grant::{self, Grant, permission_items};
use super::manifest_check::package_of;
use super::report::ExtensionReport;
use crate::output::Format;

/// What the user approved on the command line.
pub(super) struct Approval {
    pub(super) grants: Vec<Grant>,
    pub(super) grant_all: bool,
    pub(super) yes: bool,
}

pub(super) async fn install(
    host: &Host,
    client: &mut Client,
    source: &ArchiveSource,
    approval: Approval,
    format: Format,
) -> Result<ExtensionReport, ClientError> {
    let archive = fetch_extension_archive(source, &host.release_target()?).await?;
    let package = package_of(&archive.manifest, &archive.source)?;
    if format == Format::Human {
        print_summary(&archive, &package);
    }
    let has_grants = approval.grant_all || !approval.grants.is_empty();
    if !package.requested.is_empty() && !has_grants {
        return Err(grant::approval_necessary(&package));
    }
    if !approval.yes && !accepted(&package)? {
        return Err(ClientError::PermissionDenied(
            "the extension was not accepted. Nothing was written".to_owned(),
        ));
    }
    let staged = host.stage_extension_package(&archive).await?;
    let installed = async {
        let inspected = client.inspect_extension(&staged.directory).await?;
        if inspected.id != package.id || inspected.version != package.version {
            return Err(ClientError::Failed(format!(
                "the host read the extension {} {}, not {} {}",
                inspected.id, inspected.version, package.id, package.version
            )));
        }
        let granted = grant::approved(&inspected.requested, approval.grant_all, approval.grants);
        client.install_extension(&staged.directory, granted).await
    }
    .await;
    host.remove_staged_package(staged).await;
    Ok(ExtensionReport::Installed(installed?))
}

/// Prints what the archive installs on the standard error: the standard
/// output has the result.
fn print_summary(archive: &ExtensionArchive, package: &ExtensionPackage) {
    let checksum = match archive.checksum {
        ChecksumCheck::Verified => "the SHA-256 checksum is the one of SHA256SUMS",
        ChecksumCheck::NoChecksumFile => "not compared: no SHA256SUMS file is next to the archive",
    };
    let mut text = format!(
        "nomux: the archive {}\n  extension:   {} ({})\n  version:     {}\n  platform:    {}\n  \
         checksum:    {checksum}\n  permissions:",
        archive.source, package.id, package.name, package.version, archive.target
    );
    let items = permission_items(&package.requested);
    if items.is_empty() {
        text.push_str(" none");
    }
    for item in items {
        text.push_str("\n    ");
        text.push_str(&item);
    }
    text.push('\n');
    // A summary that cannot be written stops the install below: the
    // question needs a terminal.
    let _ = io::stderr().lock().write_all(text.as_bytes());
}

/// Asks the user on the terminal. Without a terminal, the answer is no:
/// `--yes` accepts without the question.
fn accepted(package: &ExtensionPackage) -> Result<bool, ClientError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(ClientError::PermissionDenied(format!(
            "the install of {} {} needs an answer, and the standard input is not a terminal. \
             Nothing was written. Add --yes to accept the extension",
            package.id, package.version
        )));
    }
    let mut stderr = io::stderr().lock();
    let _ = write!(
        stderr,
        "Install {} {} with the approved permissions? [y/N] ",
        package.id, package.version
    );
    let _ = stderr.flush();
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|error| ClientError::Failed(format!("the answer cannot be read: {error}")))?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}
