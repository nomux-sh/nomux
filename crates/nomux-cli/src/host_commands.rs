//! The `host` commands: the remote hosts of the global configuration file.

use nomux_client::{BinarySource, ClientError, Host, check_host};
use nomux_core::configuration::{HostEntry, is_valid_host_name};

use crate::arguments::HostCommand;
use crate::commands::KIND;
use crate::output::Format;
use crate::report::{AddedHost, HostDiagnosis, Report};
use crate::selected_host::{configuration, configured_host, progress_printer};

pub(crate) async fn run(command: HostCommand, format: Format) -> Result<Report, ClientError> {
    match command {
        HostCommand::Add { target, name } => add(target, name, format).await,
        HostCommand::Ls => Ok(Report::Hosts(configuration()?.hosts()?)),
        HostCommand::Rm { name } => {
            let mut configuration = configuration()?;
            if !configuration.remove_host(&name) {
                return Err(ClientError::NotFound(format!(
                    "no host has the name {name}"
                )));
            }
            configuration.save()?;
            Ok(Report::HostRemoved { name })
        }
        HostCommand::Doctor { name } => {
            let hosts = match name {
                Some(name) => vec![configured_host(&name)?],
                None => configuration()?.hosts()?,
            };
            let binaries = BinarySource::from_environment();
            let mut diagnoses = Vec::new();
            for host in hosts {
                diagnoses.push(HostDiagnosis {
                    checks: check_host(&host.ssh, &binaries, KIND).await,
                    name: host.name,
                });
            }
            Ok(Report::HostsChecked(diagnoses))
        }
    }
}

/// Connects to the host, installs the binaries, starts the daemon, and
/// saves the host only after all of that succeeded.
async fn add(target: String, name: Option<String>, format: Format) -> Result<Report, ClientError> {
    let name = match name {
        Some(name) => name,
        None => default_name(&target),
    };
    if !is_valid_host_name(&name) {
        return Err(ClientError::Failed(format!(
            "`{name}` cannot be the name of a host: a name has only the characters a to z, A to \
             Z, 0 to 9, '.', '_' and '-'. Give a name with --name"
        )));
    }
    let mut configuration = configuration()?;
    if let Some(existing) = configuration.host(&name)?
        && existing.ssh != target
    {
        return Err(ClientError::Failed(format!(
            "the host {name} already exists with the SSH destination {}. Remove it with `nomux \
             host rm {name}`, or give a different name with --name",
            existing.ssh
        )));
    }

    let binaries = BinarySource::from_environment();
    let (host, installation) =
        Host::connect_ssh(&target, &binaries, &progress_printer(format)).await?;
    let daemon = host.client(KIND).await?.host_info().await?;

    let entry = HostEntry { name, ssh: target };
    configuration.set_host(&entry);
    configuration.save()?;
    Ok(Report::HostAdded(AddedHost {
        entry,
        installation,
        daemon,
    }))
}

/// The host part of an SSH destination: without the scheme, the user and
/// the port.
fn default_name(target: &str) -> String {
    let address = target.strip_prefix("ssh://").unwrap_or(target);
    let host = address.rsplit_once('@').map_or(address, |(_, host)| host);
    let is_uri = address.len() != target.len();
    let host = match host.rsplit_once(':') {
        Some((host, _)) if is_uri => host,
        _ => host,
    };
    host.trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_name_is_the_host_part_of_the_destination() {
        assert_eq!(default_name("vps"), "vps");
        assert_eq!(default_name("deploy@vps.example.com"), "vps.example.com");
        assert_eq!(
            default_name("ssh://deploy@vps.example.com:2222"),
            "vps.example.com"
        );
        assert_eq!(default_name("ssh://vps/"), "vps");
        // An IPv6 address is not a valid name: the user gives one.
        assert!(!is_valid_host_name(&default_name("ssh://[::1]:22")));
    }
}
