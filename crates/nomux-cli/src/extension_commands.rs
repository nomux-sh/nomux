//! The `ext` commands: the extensions of a host.

mod arguments;
mod grant;
mod manifest_check;
mod payload;
mod report;

use std::path::{self, PathBuf};

use nomux_client::{Client, ClientError};

pub(crate) use arguments::ExtCommand;
pub(crate) use grant::permission_items;
pub(crate) use manifest_check::{ManifestCheck, ManifestProblem};
pub(crate) use report::ExtensionReport;

use crate::commands::KIND;
use crate::output::Format;
use crate::report::Report;
use crate::selected_host;
use grant::Grant;

/// Runs an `ext` command on the local host, or on the remote host
/// `host_name` of the configuration file.
pub(crate) async fn run(
    command: ExtCommand,
    host_name: Option<&str>,
    format: Format,
) -> Result<Report, ClientError> {
    // The check of a manifest reads a directory of this computer and needs
    // no daemon.
    if let ExtCommand::Validate { directory } = &command {
        let check = manifest_check::check(directory)?;
        return Ok(ExtensionReport::ManifestChecked(check).into());
    }
    let host = selected_host::connect(host_name, format).await?;
    let mut client = host.client(KIND).await?;
    let report = match command {
        ExtCommand::Ls => ExtensionReport::Listed(client.list_extensions().await?),
        ExtCommand::Install {
            directory,
            grant,
            grant_all,
        } => {
            let directory = package_directory(directory, host_name.is_some())?;
            install(&mut client, &directory, grant, grant_all).await?
        }
        ExtCommand::Rm { id } => {
            client.remove_extension(&id).await?;
            ExtensionReport::Removed { id }
        }
        ExtCommand::Enable { id } => {
            ExtensionReport::Enabled(client.set_extension_enabled(&id, true).await?)
        }
        ExtCommand::Disable { id } => {
            ExtensionReport::Disabled(client.set_extension_enabled(&id, false).await?)
        }
        ExtCommand::Call {
            id,
            method,
            payload,
        } => {
            let payload = payload::read(payload.as_deref())?;
            let payload = client.call_extension(&id, &method, payload).await?;
            ExtensionReport::Called {
                id,
                method,
                payload,
            }
        }
        ExtCommand::Validate { .. } => unreachable!("handled before the connection"),
    };
    Ok(report.into())
}

/// The two steps of an installation. The second step runs only with an
/// approval on the command line: the command asks no question.
async fn install(
    client: &mut Client,
    directory: &path::Path,
    grants: Vec<Grant>,
    grant_all: bool,
) -> Result<ExtensionReport, ClientError> {
    let package = client.inspect_extension(directory).await?;
    let has_approval = grant_all || !grants.is_empty();
    if !package.requested.is_empty() && !has_approval {
        return Err(grant::approval_necessary(&package));
    }
    let granted = grant::approved(&package.requested, grant_all, grants);
    let extension = client.install_extension(directory, granted).await?;
    Ok(ExtensionReport::Installed(extension))
}

/// The absolute directory of a package. On the local host, a relative path
/// is relative to the current directory of the CLI. On a remote host, the
/// path must be absolute there.
fn package_directory(directory: PathBuf, on_remote_host: bool) -> Result<PathBuf, ClientError> {
    if !on_remote_host {
        return path::absolute(&directory).map_err(|error| {
            ClientError::Failed(format!(
                "the directory {} is not valid: {error}",
                directory.display()
            ))
        });
    }
    if directory.is_absolute() {
        return Ok(directory);
    }
    Err(ClientError::Failed(format!(
        "with --host, the directory of the package must be an absolute path on the host: {} is \
         not absolute",
        directory.display()
    )))
}
