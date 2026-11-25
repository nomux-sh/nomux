//! The decision about the key that a host sends at the start of a
//! connection.

use std::future::{Future, ready};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use russh::client::Handler;
use russh::keys::{Algorithm, HashAlg, PublicKey, PublicKeyBase64, PublicKeyOrCertificate};

use super::config::StrictHostKeyChecking;
use super::known_hosts::{HostKeyVerdict, KnownHosts, add_host_key};
use super::{SshError, SshTarget};

/// Checks the host key of one connection against the files of known hosts.
/// The connection continues only with a key that is trusted.
#[derive(Debug)]
pub(super) struct HostKeyCheck {
    pub(super) known_hosts: KnownHosts,
    /// The name of the host in the files of known hosts.
    pub(super) host: String,
    pub(super) rule: StrictHostKeyChecking,
    /// The file that gets a new key, if the rule permits a new key.
    pub(super) new_keys_file: Option<PathBuf>,
    /// The destination as the user gave it, for the `ssh` command of the
    /// error message.
    pub(super) target: SshTarget,
    /// The type and the fingerprint of the key that was accepted.
    pub(super) accepted: Arc<OnceLock<String>>,
}

impl Handler for HostKeyCheck {
    type Error = SshError;

    fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = Result<bool, SshError>> + Send {
        ready(self.check(server_key).map(|()| true))
    }
}

impl HostKeyCheck {
    /// Accepts the key of the host, or gives the reason why the connection
    /// must not continue.
    fn check(&mut self, server_key: &PublicKeyOrCertificate) -> Result<(), SshError> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            return Err(SshError::Protocol(
                "the host sent a host certificate, which nomux did not ask for".to_owned(),
            ));
        };
        let key_type = key_type(key);
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let verdict = self
            .known_hosts
            .check(&self.host, &key_type, &key.public_key_bytes());
        match verdict {
            HostKeyVerdict::Trusted => {}
            HostKeyVerdict::Unknown if self.rule == StrictHostKeyChecking::Strict => {
                return Err(SshError::UnknownHostKey {
                    host: self.host.clone(),
                    accept_command: format!(
                        "ssh -o HostKeyAlgorithms={} '{}' exit",
                        host_key_algorithms(&key_type),
                        self.target
                    ),
                    key_type,
                    fingerprint,
                });
            }
            HostKeyVerdict::Unknown => {
                if let Some(file) = &self.new_keys_file {
                    add_host_key(file, &self.host, &key_type, &key.public_key_base64()).map_err(
                        |error| SshError::Configuration {
                            file: file.clone(),
                            line: 0,
                            problem: format!("the new host key cannot be added: {error}"),
                        },
                    )?;
                }
            }
            // A changed key is an error with each rule. OpenSSH permits no
            // forwarding with a changed key, and nomux needs forwarding.
            HostKeyVerdict::Changed { file, line } => {
                return Err(SshError::ChangedHostKey {
                    host: self.host.clone(),
                    key_type,
                    fingerprint,
                    file,
                    line,
                });
            }
            HostKeyVerdict::Revoked { file, line } => {
                return Err(SshError::RevokedHostKey {
                    host: self.host.clone(),
                    file,
                    line,
                });
            }
        }
        // The cell is empty: one connection has one first key exchange.
        let _ = self.accepted.set(format!("{key_type} {fingerprint}"));
        Ok(())
    }
}

/// The key type as a file of known hosts has it. An RSA key has the type
/// `ssh-rsa` with each signature algorithm.
pub(super) fn key_type(key: &PublicKey) -> String {
    match key.algorithm() {
        Algorithm::Rsa { .. } => "ssh-rsa".to_owned(),
        other => other.as_str().to_owned(),
    }
}

/// The value of the `HostKeyAlgorithms` option with which `ssh` asks for a
/// key of `key_type`. Thus `ssh` shows the fingerprint that nomux showed.
fn host_key_algorithms(key_type: &str) -> &str {
    if key_type == "ssh-rsa" {
        "rsa-sha2-512,rsa-sha2-256"
    } else {
        key_type
    }
}
