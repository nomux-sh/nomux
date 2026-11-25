//! The requests of a client for the extensions of a host.

use std::path::Path;

use nomux_proto::wire::v1::{
    ExtCallRequest, ExtInspectRequest, ExtInstallRequest, ExtListRequest, ExtRemoveRequest,
    ExtSetEnabledRequest, ext_call_response, request, response,
};

use crate::client::unexpected_result;
use crate::{Client, ClientError, Extension, ExtensionPackage, ExtensionPermissions};

impl Client {
    /// Lists the installed extensions of the host, in the sequence of their
    /// identifiers.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn list_extensions(&mut self) -> Result<Vec<Extension>, ClientError> {
        let request = request::Body::ExtList(ExtListRequest {});
        match self.call(request).await? {
            response::Result::ExtList(list) => {
                Ok(list.extensions.into_iter().map(Extension::from).collect())
            }
            _ => Err(unexpected_result()),
        }
    }

    /// The first step of an installation: asks the daemon to validate the
    /// package in `directory`, a path on the host. The result has the
    /// permissions that the manifest asks for. The daemon installs nothing.
    ///
    /// Show the permissions to the user before
    /// [`install_extension`](Self::install_extension).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the package is not valid or if the
    /// daemon cannot run it.
    pub async fn inspect_extension(
        &mut self,
        directory: &Path,
    ) -> Result<ExtensionPackage, ClientError> {
        let request = ExtInspectRequest {
            directory: host_path(directory)?,
        };
        match self.call(request::Body::ExtInspect(request)).await? {
            response::Result::ExtInspect(package) => Ok(package.into()),
            _ => Err(unexpected_result()),
        }
    }

    /// The second step of an installation: installs the package in
    /// `directory` with the permissions that the user approved. The
    /// extension gets these permissions and no others. The daemon starts no
    /// process.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the package is not valid, if
    /// `granted` has a permission that the manifest does not ask for, or if
    /// the extension is already installed.
    pub async fn install_extension(
        &mut self,
        directory: &Path,
        granted: ExtensionPermissions,
    ) -> Result<Extension, ClientError> {
        let request = ExtInstallRequest {
            directory: host_path(directory)?,
            granted: Some(granted.into()),
        };
        match self.call(request::Body::ExtInstall(request)).await? {
            response::Result::ExtInstall(installed) => installed
                .extension
                .map(Extension::from)
                .ok_or_else(|| ClientError::invalid_reply("the response has no extension")),
            _ => Err(unexpected_result()),
        }
    }

    /// Stops the extension `id` and removes its package, its state and its
    /// data from the host.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no extension has the
    /// identifier.
    pub async fn remove_extension(&mut self, id: &str) -> Result<(), ClientError> {
        let request = ExtRemoveRequest { id: id.to_owned() };
        match self.call(request::Body::ExtRemove(request)).await? {
            response::Result::ExtRemove(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    /// Enables or disables the extension `id`. The daemon stops the process
    /// of an extension that it disables. Returns the extension after the
    /// change.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no extension has the
    /// identifier.
    pub async fn set_extension_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> Result<Extension, ClientError> {
        let request = ExtSetEnabledRequest {
            id: id.to_owned(),
            enabled,
        };
        match self.call(request::Body::ExtSetEnabled(request)).await? {
            response::Result::ExtSetEnabled(changed) => changed
                .extension
                .map(Extension::from)
                .ok_or_else(|| ClientError::invalid_reply("the response has no extension")),
            _ => Err(unexpected_result()),
        }
    }

    /// Calls the method `method` of the extension `id` and returns the
    /// payload of its result. The method specifies the encoding of the two
    /// payloads. If the daemon part of the extension does not run, the
    /// daemon starts it.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ExtensionAnswered`] if the method answered
    /// with an error, [`ClientError::NotFound`] if no extension has the
    /// identifier, [`ClientError::Timeout`] if the extension did not answer
    /// in time, and [`ClientError::Failed`] if the extension is disabled,
    /// cannot start or failed.
    pub async fn call_extension(
        &mut self,
        id: &str,
        method: &str,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, ClientError> {
        let request = ExtCallRequest {
            ext: id.to_owned(),
            method: method.to_owned(),
            payload,
        };
        let response::Result::ExtCall(called) = self.call(request::Body::ExtCall(request)).await?
        else {
            return Err(unexpected_result());
        };
        match called.answer {
            Some(ext_call_response::Answer::Payload(payload)) => Ok(payload),
            Some(ext_call_response::Answer::Error(error)) => {
                Err(ClientError::ExtensionAnswered(error.into()))
            }
            None => Err(ClientError::invalid_reply("the response has no answer")),
        }
    }
}

/// The text of a path on the host for a request.
fn host_path(path: &Path) -> Result<String, ClientError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        ClientError::Failed(format!(
            "the directory {} is not valid Unicode",
            path.display()
        ))
    })
}
