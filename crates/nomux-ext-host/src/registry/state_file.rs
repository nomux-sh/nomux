//! The state file of one installed extension.
//!
//! The host owns the file. It is TOML:
//!
//! ```toml
//! enabled = false
//! disabled_reason = "the user disabled the extension"
//!
//! [permissions]
//! fs = ["/home/user/project"]
//! process = ["git"]
//! network = false
//! scopes = ["sessions:read"]
//! ```
//!
//! `enabled` is necessary. `disabled_reason` is there only when `enabled` is
//! `false`. `[permissions]` has the permissions that the user approved: a
//! key that is not there means an empty list or `false`. A key that this
//! format does not have makes the file not valid.

use std::fs;
use std::io;
use std::path::Path;

use nomux_extension::Permissions;
use serde::{Deserialize, Serialize};

use super::Enablement;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disabled_reason: Option<String>,
    #[serde(default)]
    permissions: ApprovedPermissions,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct ApprovedPermissions {
    fs: Vec<String>,
    process: Vec<String>,
    network: bool,
    scopes: Vec<String>,
}

pub(super) fn read(path: &Path) -> io::Result<(Enablement, Permissions)> {
    let text = fs::read_to_string(path)?;
    let file: StateFile =
        toml::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let enablement = if file.enabled {
        Enablement::Enabled
    } else {
        Enablement::Disabled {
            reason: file.disabled_reason.unwrap_or_default(),
        }
    };
    let permissions = Permissions {
        fs: file.permissions.fs,
        process: file.permissions.process,
        network: file.permissions.network,
        scopes: file.permissions.scopes,
    };
    Ok((enablement, permissions))
}

/// Writes the file to a temporary name and renames it, thus a reader never
/// gets a part of the file.
pub(super) fn write(path: &Path, enablement: &Enablement, granted: &Permissions) -> io::Result<()> {
    let file = StateFile {
        enabled: matches!(enablement, Enablement::Enabled),
        disabled_reason: match enablement {
            Enablement::Enabled => None,
            Enablement::Disabled { reason } => Some(reason.clone()),
        },
        permissions: ApprovedPermissions {
            fs: granted.fs.clone(),
            process: granted.process.clone(),
            network: granted.network,
            scopes: granted.scopes.clone(),
        },
    };
    let text = toml::to_string(&file)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let temporary = path.with_extension("toml.new");
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)
}
