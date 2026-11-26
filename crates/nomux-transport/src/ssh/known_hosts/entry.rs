//! One line of a file of known hosts.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

use crate::ssh::pattern::matches_list;

const HASHED_PREFIX: &str = "|1|";

/// The marker at the start of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Marker {
    None,
    /// `@revoked`: the key must not be trusted for these hosts.
    Revoked,
    /// `@cert-authority`: the key signs host certificates. This client does
    /// not use host certificates.
    CertAuthority,
}

/// The host names of a line.
#[derive(Debug)]
enum Hosts {
    /// Patterns with a comma between them.
    Patterns(String),
    /// A name that is hidden: HMAC-SHA1 of the name with a salt.
    Hashed { salt: Vec<u8>, hash: Vec<u8> },
}

#[derive(Debug)]
pub(super) struct Entry {
    pub(super) line: usize,
    pub(super) marker: Marker,
    hosts: Hosts,
    pub(super) key_type: String,
    /// The key in the encoding of the SSH protocol.
    pub(super) key: Vec<u8>,
}

impl Entry {
    /// Reads one line: `[marker] hosts key-type key [comment]`. Returns
    /// `None` for an empty line, a comment and a line that is not valid.
    pub(super) fn parse(text: &str, line: usize) -> Option<Self> {
        let mut fields = text.split_whitespace();
        let first = fields.next().filter(|field| !field.starts_with('#'))?;
        let (marker, hosts) = match first {
            "@revoked" => (Marker::Revoked, fields.next()?),
            "@cert-authority" => (Marker::CertAuthority, fields.next()?),
            hosts => (Marker::None, hosts),
        };
        let hosts = match hosts.strip_prefix(HASHED_PREFIX) {
            Some(hashed) => {
                let (salt, hash) = hashed.split_once('|')?;
                Hosts::Hashed {
                    salt: STANDARD.decode(salt).ok()?,
                    hash: STANDARD.decode(hash).ok()?,
                }
            }
            None => Hosts::Patterns(hosts.to_owned()),
        };
        Some(Self {
            line,
            marker,
            hosts,
            key_type: fields.next()?.to_owned(),
            key: STANDARD.decode(fields.next()?).ok()?,
        })
    }

    /// Returns `true` if the line is for `host`.
    pub(super) fn names(&self, host: &str) -> bool {
        match &self.hosts {
            Hosts::Patterns(patterns) => matches_list(patterns.split(','), host),
            Hosts::Hashed { salt, hash } => {
                Hmac::<Sha1>::new_from_slice(salt).is_ok_and(|mut mac| {
                    mac.update(host.as_bytes());
                    mac.verify_slice(hash).is_ok()
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hashed_name_matches_only_its_host() {
        // `ssh-keygen -H` made this line for the host `192.0.2.61`.
        let entry = Entry::parse(
            "|1|Mro6G1AXl+HitwBWwHyej6Scd98=|RMQw28Lxbz5jnalYvyarQ3+uH0w= ssh-ed25519 \
             AAAAC3NzaC1lZDI1NTE5AAAAIJG3OLmKvR+sGTLFUIGCan95CJPxX1xdaevc1zM4xZKU",
            1,
        )
        .unwrap();

        assert!(entry.names("192.0.2.61"));
        assert!(!entry.names("192.0.2.62"));
    }

    #[test]
    fn a_line_that_is_not_valid_is_ignored() {
        assert!(Entry::parse("# vps ssh-ed25519 AAAA", 1).is_none());
        assert!(Entry::parse("vps ssh-ed25519", 1).is_none());
        assert!(Entry::parse("vps ssh-ed25519 not-base64!", 1).is_none());
        assert!(Entry::parse("@revoked", 1).is_none());
    }
}
