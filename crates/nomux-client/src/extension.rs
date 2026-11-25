//! The extensions of a host, as a client sees them.

use std::fmt;

use nomux_proto::wire::v1 as wire;

/// The state of an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionState {
    /// The extension is enabled and its daemon part does not run. The next
    /// call starts it.
    Stopped,
    /// The process of the daemon part started and is not ready.
    Starting,
    /// The daemon part runs.
    Running,
    /// The daemon part failed and does not run. After a pause, the next
    /// call starts it again.
    Failed,
    /// The daemon does not start the extension.
    Disabled,
    /// A state from a later protocol version.
    Unknown,
}

/// The permissions of an extension: the ones that its manifest asks for,
/// or the ones that the user approved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtensionPermissions {
    /// The directories that the extension reads and writes.
    pub fs: Vec<String>,
    /// The programs that the extension starts.
    pub process: Vec<String>,
    /// `true` if the extension opens network connections.
    pub network: bool,
    /// The scopes of host services, for example `sessions:read`.
    pub scopes: Vec<String>,
}

impl ExtensionPermissions {
    /// Returns `true` if the value has no permission.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fs.is_empty() && self.process.is_empty() && !self.network && self.scopes.is_empty()
    }
}

/// One installed extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// The identifier of the extension, for example `nomux.git`.
    pub id: String,
    /// The name of the extension for the user.
    pub name: String,
    /// The version of the extension.
    pub version: String,
    /// The state of the extension.
    pub state: ExtensionState,
    /// Why the extension is disabled, or the failure of its process. `None`
    /// in the other states.
    pub state_reason: Option<String>,
    /// The permissions that the user approved.
    pub granted: ExtensionPermissions,
}

/// A package that is not installed: what its manifest says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionPackage {
    /// The identifier in the manifest.
    pub id: String,
    /// The name in the manifest.
    pub name: String,
    /// The version in the manifest.
    pub version: String,
    /// The permissions that the manifest asks for.
    pub requested: ExtensionPermissions,
}

/// Why a method of an extension did not give a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionErrorCode {
    /// The method failed, and no other code gives the reason. Also a code
    /// from a later protocol version.
    Failed,
    /// The extension does not have the method, or the item that the call
    /// names does not exist.
    NotFound,
    /// The extension did not complete the call in the time that it permits.
    Timeout,
    /// The extension does not have the permission that the call needs.
    PermissionDenied,
    /// The extension cannot read the payload of the call.
    InvalidArgument,
    /// The extension stopped the call.
    Cancelled,
}

/// The error with which an extension answered a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCallError {
    /// The reason for the failure.
    pub code: ExtensionErrorCode,
    /// A message in English for a person. The text is not stable.
    pub message: String,
}

impl fmt::Display for ExtensionCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "the extension answered with an error: {}",
            self.message
        )
    }
}

impl From<wire::ExtensionPermissions> for ExtensionPermissions {
    fn from(permissions: wire::ExtensionPermissions) -> Self {
        Self {
            fs: permissions.fs,
            process: permissions.process,
            network: permissions.network,
            scopes: permissions.scopes,
        }
    }
}

impl From<ExtensionPermissions> for wire::ExtensionPermissions {
    fn from(permissions: ExtensionPermissions) -> Self {
        Self {
            fs: permissions.fs,
            process: permissions.process,
            network: permissions.network,
            scopes: permissions.scopes,
        }
    }
}

impl From<wire::Extension> for Extension {
    fn from(extension: wire::Extension) -> Self {
        let state = match wire::ExtensionState::try_from(extension.state) {
            Ok(wire::ExtensionState::Stopped) => ExtensionState::Stopped,
            Ok(wire::ExtensionState::Starting) => ExtensionState::Starting,
            Ok(wire::ExtensionState::Running) => ExtensionState::Running,
            Ok(wire::ExtensionState::Failed) => ExtensionState::Failed,
            Ok(wire::ExtensionState::Disabled) => ExtensionState::Disabled,
            Ok(wire::ExtensionState::Unspecified) | Err(_) => ExtensionState::Unknown,
        };
        Self {
            id: extension.id,
            name: extension.name,
            version: extension.version,
            state,
            state_reason: Some(extension.state_reason).filter(|reason| !reason.is_empty()),
            granted: extension.granted.unwrap_or_default().into(),
        }
    }
}

impl From<wire::ExtInspectResponse> for ExtensionPackage {
    fn from(package: wire::ExtInspectResponse) -> Self {
        Self {
            id: package.id,
            name: package.name,
            version: package.version,
            requested: package.requested.unwrap_or_default().into(),
        }
    }
}

impl From<wire::ExtCallError> for ExtensionCallError {
    fn from(error: wire::ExtCallError) -> Self {
        use wire::ExtCallErrorCode as Wire;
        let code = match Wire::try_from(error.code) {
            Ok(Wire::NotFound) => ExtensionErrorCode::NotFound,
            Ok(Wire::Timeout) => ExtensionErrorCode::Timeout,
            Ok(Wire::PermissionDenied) => ExtensionErrorCode::PermissionDenied,
            Ok(Wire::InvalidArgument) => ExtensionErrorCode::InvalidArgument,
            Ok(Wire::Cancelled) => ExtensionErrorCode::Cancelled,
            // The extension protocol uses the version code only in its
            // handshake.
            Ok(Wire::Failed | Wire::VersionIncompatible | Wire::Unspecified) | Err(_) => {
                ExtensionErrorCode::Failed
            }
        };
        Self {
            code,
            message: error.message,
        }
    }
}
