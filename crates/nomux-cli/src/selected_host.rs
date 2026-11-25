//! The host of a command: the local host, or the remote host that the
//! `--host` option names in the global configuration file.

use std::io::{self, Write};

use nomux_client::{BinarySource, BootstrapStep, ClientError, Host};
use nomux_core::configuration::{ConfigurationFile, HostEntry, default_configuration_path};

use crate::output::Format;

/// Reads the global configuration file. A file that does not exist is an
/// empty configuration.
pub(crate) fn configuration() -> Result<ConfigurationFile, ClientError> {
    let path = default_configuration_path().ok_or_else(|| {
        ClientError::Failed(
            "the HOME variable is not set, thus the configuration file has no path".to_owned(),
        )
    })?;
    Ok(ConfigurationFile::load(&path)?)
}

/// The host `name` of the configuration file.
pub(crate) fn configured_host(name: &str) -> Result<HostEntry, ClientError> {
    configuration()?.host(name)?.ok_or_else(|| {
        ClientError::NotFound(format!(
            "no host has the name {name}. `nomux host ls` shows the hosts, and `nomux host add` \
             adds one"
        ))
    })
}

/// Gives the host of a command. For a remote host, connects to it and
/// makes sure that it has the nomux binaries.
pub(crate) async fn connect(name: Option<&str>, format: Format) -> Result<Host, ClientError> {
    let Some(name) = name else {
        return Host::local();
    };
    let entry = configured_host(name)?;
    let (host, _) = Host::connect_ssh(
        &entry.ssh,
        &BinarySource::from_environment(),
        &progress_printer(format),
    )
    .await?;
    Ok(host)
}

/// Prints the steps of a bootstrap that take time on the standard error.
/// Only the text output has them: the standard error of the JSON output is
/// one document.
pub(crate) fn progress_printer(format: Format) -> impl Fn(BootstrapStep<'_>) + Send + Sync {
    move |step| {
        if format != Format::Human {
            return;
        }
        let line = match step {
            BootstrapStep::Downloading { url } => format!("nomux: downloading {url}\n"),
            BootstrapStep::Uploading { target, bytes } => format!(
                "nomux: installing nomux {} for {target} on the host ({} KiB)\n",
                env!("CARGO_PKG_VERSION"),
                bytes / 1024
            ),
        };
        // A progress line that cannot be written stops no command.
        let _ = io::stderr().lock().write_all(line.as_bytes());
    }
}
