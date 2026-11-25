//! The authentication of the user: the keys of ssh-agent, then the key
//! files. nomux has no password and asks for none.

use std::io;
use std::path::Path;
use std::sync::Arc;

use russh::client::{AuthResult, Handle};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::AgentClient;
use russh::keys::{
    Error as KeyError, HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyBase64, load_public_key,
    load_secret_key,
};

use super::config::{HostSettings, IdentityAgent};
use super::host_key_check::HostKeyCheck;
use super::{SshEnvironment, SshError};

type Session = Handle<HostKeyCheck>;

/// One attempt of an authentication: the keys that were tried, and the
/// reasons why a key was not usable.
struct Attempt<'a> {
    session: &'a mut Session,
    user: &'a str,
    /// The public keys that the host refused, in their encoded form.
    refused: Vec<Vec<u8>>,
    notes: Vec<String>,
}

/// Authenticates `session` as the user of `settings`.
pub(super) async fn authenticate(
    session: &mut Session,
    settings: &HostSettings,
    environment: &SshEnvironment,
) -> Result<(), SshError> {
    let mut attempt = Attempt {
        session,
        user: &settings.user,
        refused: Vec::new(),
        notes: Vec::new(),
    };
    let accepted = attempt.with_agent(settings, environment).await?
        || attempt.with_key_files(settings).await?;
    if accepted {
        return Ok(());
    }
    if attempt.refused.is_empty() && attempt.notes.is_empty() {
        attempt
            .notes
            .push("ssh-agent has no key and no key file exists".to_owned());
    }
    Err(SshError::Authentication {
        user: settings.user.clone(),
        host: settings.host_name.clone(),
        notes: attempt.notes,
    })
}

impl Attempt<'_> {
    /// Tries the keys of the agent. With `IdentitiesOnly`, only the keys
    /// that a key file of the settings names.
    async fn with_agent(
        &mut self,
        settings: &HostSettings,
        environment: &SshEnvironment,
    ) -> Result<bool, SshError> {
        let socket = match &settings.identity_agent {
            IdentityAgent::Disabled => return Ok(false),
            IdentityAgent::Socket(socket) => socket,
            IdentityAgent::Environment => match &environment.agent_socket {
                Some(socket) => socket,
                None => return Ok(false),
            },
        };
        let mut agent = match AgentClient::connect_uds(socket).await {
            Ok(agent) => agent,
            Err(error) => {
                self.notes.push(format!(
                    "ssh-agent at {} cannot be used: {error}",
                    socket.display()
                ));
                return Ok(false);
            }
        };
        let identities = match agent.request_identities().await {
            Ok(identities) => identities,
            Err(error) => {
                self.notes
                    .push(format!("ssh-agent did not give its keys: {error}"));
                return Ok(false);
            }
        };
        let permitted = settings
            .identities_only
            .then(|| public_keys_of(&settings.identity_files));
        for identity in identities {
            let AgentIdentity::PublicKey { key, .. } = identity else {
                // A certificate needs `CertificateFile`, which is an error
                // of the configuration.
                continue;
            };
            let encoded = key.public_key_bytes();
            if permitted
                .as_ref()
                .is_some_and(|permitted| !permitted.contains(&encoded))
            {
                continue;
            }
            let hash = self.hash_for(&key).await?;
            let result = self
                .session
                .authenticate_publickey_with(self.user, key, hash, &mut agent)
                .await;
            match result {
                Ok(AuthResult::Success) => return Ok(true),
                Ok(AuthResult::Failure { .. }) => self.refused.push(encoded),
                Err(error) => self
                    .notes
                    .push(format!("ssh-agent did not sign with a key: {error}")),
            }
        }
        Ok(false)
    }

    /// Tries the key files that exist and have no passphrase.
    async fn with_key_files(&mut self, settings: &HostSettings) -> Result<bool, SshError> {
        for file in &settings.identity_files {
            let key = match load_secret_key(file, None) {
                Ok(key) => key,
                Err(KeyError::IO(error)) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(KeyError::KeyIsEncrypted) => {
                    self.notes.push(format!(
                        "The key {path} has a passphrase: add it to ssh-agent with `ssh-add {path}`",
                        path = file.display()
                    ));
                    continue;
                }
                Err(error) => {
                    self.notes.push(format!(
                        "The key {} cannot be used: {error}",
                        file.display()
                    ));
                    continue;
                }
            };
            let encoded = key.public_key_bytes();
            if self.refused.contains(&encoded) {
                continue;
            }
            let hash = self.hash_for(key.public_key()).await?;
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
            match self.session.authenticate_publickey(self.user, key).await? {
                AuthResult::Success => return Ok(true),
                AuthResult::Failure { .. } => self.refused.push(encoded),
            }
        }
        Ok(false)
    }

    /// The hash of the signature of an RSA key: the best one that the host
    /// supports. Each other key type has one signature algorithm.
    async fn hash_for(&self, key: &PublicKey) -> Result<Option<HashAlg>, SshError> {
        if !key.algorithm().is_rsa() {
            return Ok(None);
        }
        // A host that does not name its algorithms gets SHA-256: OpenSSH
        // refuses the SHA-1 signature by default.
        Ok(self
            .session
            .best_supported_rsa_hash()
            .await?
            .unwrap_or(Some(HashAlg::Sha256)))
    }
}

/// The public keys of the key files, in their encoded form: from the `.pub`
/// file, or from a key file without a passphrase.
fn public_keys_of(identity_files: &[impl AsRef<Path>]) -> Vec<Vec<u8>> {
    identity_files
        .iter()
        .filter_map(|file| {
            let file = file.as_ref();
            let mut public_file = file.as_os_str().to_owned();
            public_file.push(".pub");
            load_public_key(public_file)
                .ok()
                .or_else(|| Some(load_secret_key(file, None).ok()?.public_key().clone()))
        })
        .map(|key| key.public_key_bytes())
        .collect()
}
