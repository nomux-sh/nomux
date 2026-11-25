//! The destination of an SSH connection, as a user writes it.

use std::fmt;
use std::str::FromStr;

use super::SshError;

const URI_SCHEME: &str = "ssh://";

/// An SSH destination: `[user@]host` or `ssh://[user@]host[:port]`, the two
/// forms that the `ssh` command accepts. `host` can be a `Host` name of
/// `~/.ssh/config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    /// The user on the host, if the destination has one.
    pub user: Option<String>,
    /// The host name, the address or the `Host` name of the configuration.
    pub host: String,
    /// The port, if the destination has one.
    pub port: Option<u16>,
}

impl SshTarget {
    /// Reads one entry of a `ProxyJump` list: `[user@]host[:port]` or an
    /// `ssh://` address.
    pub(crate) fn parse_jump(text: &str) -> Result<Self, SshError> {
        if text.starts_with(URI_SCHEME) {
            return text.parse();
        }
        let (user, address) = split_user(text);
        let (host, port) = split_port(address).map_err(|problem| invalid(text, problem))?;
        Self::new(text, user, host, port)
    }

    fn new(
        text: &str,
        user: Option<&str>,
        host: &str,
        port: Option<u16>,
    ) -> Result<Self, SshError> {
        let is_plain = |part: &str| {
            !part.is_empty()
                && !part.starts_with('-')
                && !part
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control())
        };
        if !is_plain(host) {
            return Err(invalid(text, "the host is empty or is not a host name"));
        }
        if user.is_some_and(|user| !is_plain(user)) {
            return Err(invalid(text, "the user is empty or is not a user name"));
        }
        Ok(Self {
            user: user.map(str::to_owned),
            host: host.to_owned(),
            port,
        })
    }
}

impl FromStr for SshTarget {
    type Err = SshError;

    fn from_str(text: &str) -> Result<Self, SshError> {
        let Some(address) = text.strip_prefix(URI_SCHEME) else {
            let (user, host) = split_user(text);
            return Self::new(text, user, host, None);
        };
        let address = address.strip_suffix('/').unwrap_or(address);
        let (user, address) = split_user(address);
        let (host, port) = split_port(address).map_err(|problem| invalid(text, problem))?;
        Self::new(text, user, host, port)
    }
}

/// The form that the `ssh` command accepts.
impl fmt::Display for SshTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let user = self
            .user
            .as_ref()
            .map_or_else(String::new, |user| format!("{user}@"));
        match self.port {
            Some(port) if self.host.contains(':') => {
                write!(formatter, "{URI_SCHEME}{user}[{}]:{port}", self.host)
            }
            Some(port) => write!(formatter, "{URI_SCHEME}{user}{}:{port}", self.host),
            None => write!(formatter, "{user}{}", self.host),
        }
    }
}

/// The user is the part before the last `@`: a user name can contain `@`.
fn split_user(text: &str) -> (Option<&str>, &str) {
    match text.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, text),
    }
}

/// Splits `host[:port]`. An IPv6 address with a port is in brackets.
fn split_port(address: &str) -> Result<(&str, Option<u16>), &'static str> {
    let (host, port) = if let Some(bracketed) = address.strip_prefix('[') {
        let (host, rest) = bracketed
            .split_once(']')
            .ok_or("the address has '[' without ']'")?;
        (host, rest.strip_prefix(':'))
    } else {
        match address.rsplit_once(':') {
            // More than one ':' without brackets is an IPv6 address.
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            _ => (address, None),
        }
    };
    let port = port
        .map(|port| port.parse::<u16>().ok().filter(|port| *port != 0))
        .map(|port| port.ok_or("the port is not a number from 1 to 65535"))
        .transpose()?;
    Ok((host, port))
}

fn invalid(text: &str, problem: &'static str) -> SshError {
    SshError::InvalidTarget {
        target: text.to_owned(),
        problem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(user: Option<&str>, host: &str, port: Option<u16>) -> SshTarget {
        SshTarget {
            user: user.map(str::to_owned),
            host: host.to_owned(),
            port,
        }
    }

    #[test]
    fn the_two_forms_of_the_ssh_command_are_read() {
        assert_eq!(
            "vps".parse::<SshTarget>().unwrap(),
            target(None, "vps", None)
        );
        assert_eq!(
            "deploy@vps.example.com".parse::<SshTarget>().unwrap(),
            target(Some("deploy"), "vps.example.com", None)
        );
        assert_eq!(
            "ssh://deploy@vps.example.com:2222"
                .parse::<SshTarget>()
                .unwrap(),
            target(Some("deploy"), "vps.example.com", Some(2222))
        );
        assert_eq!(
            "ssh://[::1]:2222".parse::<SshTarget>().unwrap(),
            target(None, "::1", Some(2222))
        );
        // Without the scheme, the `ssh` command has no port in a destination.
        assert_eq!(
            "vps:2222".parse::<SshTarget>().unwrap(),
            target(None, "vps:2222", None)
        );
    }

    #[test]
    fn a_destination_that_the_ssh_command_reads_as_an_option_is_refused() {
        assert!("-oProxyCommand=evil".parse::<SshTarget>().is_err());
        assert!("user@-host".parse::<SshTarget>().is_err());
        assert!("".parse::<SshTarget>().is_err());
        assert!("a b".parse::<SshTarget>().is_err());
        assert!("ssh://host:0".parse::<SshTarget>().is_err());
        assert!("ssh://host:port".parse::<SshTarget>().is_err());
    }

    #[test]
    fn the_text_form_is_a_destination_with_the_same_parts() {
        for text in [
            "vps",
            "deploy@vps",
            "ssh://deploy@vps:2222",
            "ssh://[::1]:2222",
        ] {
            let parsed: SshTarget = text.parse().unwrap();
            assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn a_jump_host_can_have_a_port_without_the_scheme() {
        assert_eq!(
            SshTarget::parse_jump("deploy@bastion:2222").unwrap(),
            target(Some("deploy"), "bastion", Some(2222))
        );
        assert_eq!(
            SshTarget::parse_jump("bastion").unwrap(),
            target(None, "bastion", None)
        );
    }
}
