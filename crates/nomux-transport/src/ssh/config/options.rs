//! The options that the configuration sets for one host, and the settings
//! that they make together with the defaults.
//!
//! OpenSSH uses the first value that it gets for an option. `IdentityFile`
//! is the exception: each value adds a file.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::lexer::Directive;
use super::settings::{HostSettings, IdentityAgent, StrictHostKeyChecking};
use super::tokens::{TokenValues, expand, expand_path};
use crate::ssh::{SshEnvironment, SshError, SshTarget};

const DEFAULT_PORT: u16 = 22;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// nomux must learn in a short time that a connection is dead, because an
/// attach connects again then. OpenSSH has no interval by default.
const DEFAULT_SERVER_ALIVE_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_SERVER_ALIVE_COUNT_MAX: usize = 3;
const NONE: &str = "none";
/// The key files that OpenSSH tries when the configuration names none.
const DEFAULT_IDENTITY_FILES: [&str; 5] = [
    "id_rsa",
    "id_ecdsa",
    "id_ecdsa_sk",
    "id_ed25519",
    "id_ed25519_sk",
];
const DEFAULT_USER_KNOWN_HOSTS_FILES: [&str; 2] = ["known_hosts", "known_hosts2"];
const DEFAULT_GLOBAL_KNOWN_HOSTS_FILES: [&str; 2] =
    ["/etc/ssh/ssh_known_hosts", "/etc/ssh/ssh_known_hosts2"];

/// Where a line of the configuration is.
#[derive(Debug, Clone, Copy)]
pub(super) struct Location<'a> {
    pub(super) file: &'a Path,
    pub(super) line: usize,
}

impl Location<'_> {
    pub(super) fn error(self, problem: impl Into<String>) -> SshError {
        SshError::Configuration {
            file: self.file.to_owned(),
            line: self.line,
            problem: problem.into(),
        }
    }

    fn unsupported(self, option: &str) -> SshError {
        SshError::UnsupportedOption {
            option: option.to_owned(),
            file: self.file.to_owned(),
            line: self.line,
        }
    }
}

/// How the configuration reaches the host.
#[derive(Debug)]
enum Proxy {
    /// `ProxyJump` with its value.
    Jump(String),
    /// `ProxyCommand`, which this client does not have.
    Command(SshError),
    /// `ProxyJump none` or `ProxyCommand none`.
    Direct,
}

/// The values that the configuration gave until now.
#[derive(Debug, Default)]
pub(super) struct Options {
    host_name: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_files: Option<Vec<String>>,
    identities_only: Option<bool>,
    identity_agent: Option<String>,
    proxy: Option<Proxy>,
    user_known_hosts_files: Option<Vec<String>>,
    global_known_hosts_files: Option<Vec<String>>,
    strict_host_key_checking: Option<StrictHostKeyChecking>,
    host_key_alias: Option<String>,
    connect_timeout: Option<u64>,
    server_alive_interval: Option<u64>,
    server_alive_count_max: Option<usize>,
    /// The first option that applies to the host and that this client
    /// does not have.
    unsupported: Option<SshError>,
}

impl Options {
    /// The host name that the configuration gave until now. `Match host`
    /// uses it.
    pub(super) fn host_name(&self) -> Option<&str> {
        self.host_name.as_deref()
    }

    /// The user that the configuration gave until now. `Match user` uses
    /// it.
    pub(super) fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    /// Applies one line that is active for the host.
    pub(super) fn apply(
        &mut self,
        directive: &Directive,
        location: Location<'_>,
    ) -> Result<(), SshError> {
        let keyword = directive.keyword.as_str();
        let first = || first_argument(directive, location);
        match keyword {
            "hostname" => set_once(&mut self.host_name, first()?),
            "user" => set_once(&mut self.user, first()?),
            "port" => {
                let port = first()?;
                let port = port
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| {
                        location.error(format!("`Port {port}`: not a number from 1 to 65535"))
                    })?;
                set_once(&mut self.port, port);
            }
            "identityfile" => self.identity_files.get_or_insert_default().push(first()?),
            "identitiesonly" => {
                set_once(&mut self.identities_only, flag(&first()?, location)?);
            }
            "identityagent" => set_once(&mut self.identity_agent, first()?),
            "proxyjump" | "proxycommand" => {
                let proxy = match (keyword, first()?) {
                    (_, value) if is_none(&value) => Proxy::Direct,
                    ("proxyjump", jump_hosts) => Proxy::Jump(jump_hosts),
                    _ => Proxy::Command(location.unsupported("ProxyCommand")),
                };
                set_once(&mut self.proxy, proxy);
            }
            "userknownhostsfile"
            | "globalknownhostsfile"
            | "stricthostkeychecking"
            | "hostkeyalias" => self.apply_host_key_option(directive, location)?,
            "connecttimeout" | "serveraliveinterval" | "serveralivecountmax" => {
                self.apply_time_option(directive, location)?;
            }
            _ => self.note_if_unsupported(directive, location)?,
        }
        Ok(())
    }

    /// The options for the check of the host key.
    fn apply_host_key_option(
        &mut self,
        directive: &Directive,
        location: Location<'_>,
    ) -> Result<(), SshError> {
        let first = first_argument(directive, location)?;
        match directive.keyword.as_str() {
            "userknownhostsfile" => {
                set_once(
                    &mut self.user_known_hosts_files,
                    directive.arguments.clone(),
                );
            }
            "globalknownhostsfile" => {
                set_once(
                    &mut self.global_known_hosts_files,
                    directive.arguments.clone(),
                );
            }
            "hostkeyalias" => set_once(&mut self.host_key_alias, first),
            _ => {
                let rule = match first.to_ascii_lowercase().as_str() {
                    "yes" | "ask" => StrictHostKeyChecking::Strict,
                    "accept-new" => StrictHostKeyChecking::AcceptNew,
                    "no" | "off" => StrictHostKeyChecking::Off,
                    other => {
                        return Err(location.error(format!(
                            "`StrictHostKeyChecking {other}`: not yes, ask, accept-new, no or off"
                        )));
                    }
                };
                set_once(&mut self.strict_host_key_checking, rule);
            }
        }
        Ok(())
    }

    /// The options that are a number of seconds or a count.
    fn apply_time_option(
        &mut self,
        directive: &Directive,
        location: Location<'_>,
    ) -> Result<(), SshError> {
        let keyword = directive.keyword.as_str();
        let text = first_argument(directive, location)?;
        let number = if keyword == "connecttimeout" && is_none(&text) {
            0
        } else {
            text.parse::<u64>()
                .map_err(|_| location.error(format!("`{keyword} {text}`: not a number")))?
        };
        match keyword {
            "connecttimeout" => set_once(&mut self.connect_timeout, number),
            "serveraliveinterval" => set_once(&mut self.server_alive_interval, number),
            _ => {
                let count = usize::try_from(number).unwrap_or(usize::MAX);
                set_once(&mut self.server_alive_count_max, count);
            }
        }
        Ok(())
    }

    /// Keeps the first option that applies to the host and that this client
    /// does not have. Each other option changes nothing that this client
    /// does: `docs/remote.md` has the list.
    fn note_if_unsupported(
        &mut self,
        directive: &Directive,
        location: Location<'_>,
    ) -> Result<(), SshError> {
        let (option, is_off): (&str, &[&str]) = match directive.keyword.as_str() {
            "certificatefile" => ("CertificateFile", &[NONE]),
            "pkcs11provider" => ("PKCS11Provider", &[NONE]),
            "securitykeyprovider" => ("SecurityKeyProvider", &[NONE]),
            "knownhostscommand" => ("KnownHostsCommand", &[NONE]),
            "canonicalizehostname" => ("CanonicalizeHostname", &["no", NONE]),
            "bindaddress" => ("BindAddress", &[]),
            "bindinterface" => ("BindInterface", &[]),
            _ => return Ok(()),
        };
        let value = first_argument(directive, location)?.to_ascii_lowercase();
        if !is_off.contains(&value.as_str()) {
            set_once(&mut self.unsupported, location.unsupported(option));
        }
        Ok(())
    }

    /// Makes the settings of the connection. A value of the destination
    /// comes before a value of the configuration, as on the command line of
    /// `ssh`.
    pub(super) fn finish(
        self,
        target: &SshTarget,
        environment: &SshEnvironment,
        configuration_file: &Path,
    ) -> Result<HostSettings, SshError> {
        if let Some(unsupported) = self.unsupported {
            return Err(unsupported);
        }
        let invalid = |problem: String| SshError::Configuration {
            file: configuration_file.to_owned(),
            line: 0,
            problem,
        };
        let port = target.port.or(self.port).unwrap_or(DEFAULT_PORT);
        let user = target
            .user
            .clone()
            .or(self.user)
            .unwrap_or_else(|| environment.user.clone());
        let mut values = TokenValues {
            home: &environment.home,
            local_user: &environment.user,
            host_name: &target.host,
            original_host: &target.host,
            port,
            remote_user: &user,
            host_key_alias: &target.host,
        };
        let host_name = match &self.host_name {
            Some(host_name) => expand(host_name, &values).map_err(invalid)?,
            None => target.host.clone(),
        };
        let host_key_alias = self.host_key_alias.unwrap_or_else(|| host_name.clone());
        values.host_name = &host_name;
        values.host_key_alias = &host_key_alias;

        let ssh_directory = environment.ssh_directory();
        let in_ssh_directory =
            |names: &[&str]| names.iter().map(|name| ssh_directory.join(name)).collect();
        let paths = |files: Option<Vec<String>>, default: Vec<PathBuf>| match files {
            Some(files) => expand_paths(&files, &values).map_err(invalid),
            None => Ok(default),
        };
        Ok(HostSettings {
            identity_files: paths(
                self.identity_files,
                in_ssh_directory(&DEFAULT_IDENTITY_FILES),
            )?,
            user_known_hosts_files: paths(
                self.user_known_hosts_files,
                in_ssh_directory(&DEFAULT_USER_KNOWN_HOSTS_FILES),
            )?,
            global_known_hosts_files: paths(
                self.global_known_hosts_files,
                DEFAULT_GLOBAL_KNOWN_HOSTS_FILES
                    .iter()
                    .map(PathBuf::from)
                    .collect(),
            )?,
            identity_agent: identity_agent(self.identity_agent.as_deref(), &values)
                .map_err(invalid)?,
            proxy_jump: match self.proxy {
                None | Some(Proxy::Direct) => Vec::new(),
                Some(Proxy::Command(unsupported)) => return Err(unsupported),
                Some(Proxy::Jump(jump_hosts)) => jump_hosts
                    .split(',')
                    .map(SshTarget::parse_jump)
                    .collect::<Result<_, _>>()?,
            },
            identities_only: self.identities_only.unwrap_or(false),
            strict_host_key_checking: self
                .strict_host_key_checking
                .unwrap_or(StrictHostKeyChecking::Strict),
            connect_timeout: match self.connect_timeout {
                None | Some(0) => DEFAULT_CONNECT_TIMEOUT,
                Some(seconds) => Duration::from_secs(seconds),
            },
            server_alive_interval: match self.server_alive_interval {
                None => Some(DEFAULT_SERVER_ALIVE_INTERVAL),
                Some(0) => None,
                Some(seconds) => Some(Duration::from_secs(seconds)),
            },
            server_alive_count_max: self
                .server_alive_count_max
                .unwrap_or(DEFAULT_SERVER_ALIVE_COUNT_MAX),
            port,
            user,
            host_name,
            host_key_alias,
        })
    }
}

/// The first argument of a line. Each option of this module needs one.
fn first_argument(directive: &Directive, location: Location<'_>) -> Result<String, SshError> {
    directive
        .arguments
        .first()
        .cloned()
        .ok_or_else(|| location.error(format!("`{}` needs a value", directive.keyword)))
}

/// Expands a list of files. The list is empty if a file is `none`.
fn expand_paths(files: &[String], values: &TokenValues<'_>) -> Result<Vec<PathBuf>, String> {
    if files.iter().any(|file| is_none(file)) {
        return Ok(Vec::new());
    }
    files
        .iter()
        .map(|file| expand_path(file, values).map(PathBuf::from))
        .collect()
}

/// The agent of an `IdentityAgent` value: `none`, `SSH_AUTH_SOCK`, a
/// variable with `$` before its name, or the path of a socket.
fn identity_agent(value: Option<&str>, values: &TokenValues<'_>) -> Result<IdentityAgent, String> {
    let agent = match value {
        None | Some("SSH_AUTH_SOCK") => return Ok(IdentityAgent::Environment),
        Some(agent) if is_none(agent) => return Ok(IdentityAgent::Disabled),
        Some(agent) => agent,
    };
    let socket = match agent
        .strip_prefix('$')
        .filter(|name| !name.starts_with('{'))
    {
        Some(variable) => env::var(variable)
            .map_err(|_| format!("`IdentityAgent {agent}`: the variable {variable} is not set"))?,
        None => expand_path(agent, values)?,
    };
    Ok(IdentityAgent::Socket(PathBuf::from(socket)))
}

fn set_once<T>(slot: &mut Option<T>, value: T) {
    if slot.is_none() {
        *slot = Some(value);
    }
}

fn is_none(text: &str) -> bool {
    text.eq_ignore_ascii_case(NONE)
}

fn flag(text: &str, location: Location<'_>) -> Result<bool, SshError> {
    match text.to_ascii_lowercase().as_str() {
        "yes" | "true" => Ok(true),
        "no" | "false" => Ok(false),
        other => Err(location.error(format!("`{other}`: not yes or no"))),
    }
}
