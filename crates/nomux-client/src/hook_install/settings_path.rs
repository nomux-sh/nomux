//! The path of the settings file of a tool on this computer.

use std::env;
use std::path::{Component, Path, PathBuf};

use crate::{ClientError, HookInstallPlan, HookPlatform};

/// The prefix of a settings path: the home directory of the user.
const HOME_PREFIX: &str = "~/";

#[cfg(target_os = "macos")]
const THIS_PLATFORM: HookPlatform = HookPlatform::Macos;
#[cfg(not(target_os = "macos"))]
const THIS_PLATFORM: HookPlatform = HookPlatform::Linux;

/// The settings file of `plan` on this platform, as an absolute path.
///
/// # Errors
///
/// Returns [`ClientError::Failed`] if the plan has no path for this
/// platform, or if the path is not below the home directory: the adapter
/// gives the path, thus it is input from outside.
pub(super) fn settings_path(plan: &HookInstallPlan) -> Result<PathBuf, ClientError> {
    let path = plan
        .settings_paths
        .iter()
        .find(|path| path.platform == THIS_PLATFORM)
        .ok_or_else(|| {
            ClientError::Failed(format!(
                "the adapter {} gives no settings file for this platform",
                plan.extension_id
            ))
        })?;
    below_home(&path.path).ok_or_else(|| {
        ClientError::Failed(format!(
            "the adapter {} gives the settings path `{}`, which is not a path below `~/`",
            plan.extension_id, path.path
        ))
    })
}

/// `path` with the home directory for `~/`. `None` if `path` does not start
/// with `~/`, has a `.` or `..` part, or `HOME` is not set.
fn below_home(path: &str) -> Option<PathBuf> {
    let relative = Path::new(path.strip_prefix(HOME_PREFIX)?);
    let normal = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    let has_dot = path.split('/').any(|part| part == "." || part == "..");
    if !normal || has_dot || relative.as_os_str().is_empty() || path.contains('\0') {
        return None;
    }
    let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settings_path_stays_below_the_home_directory() {
        let home = env::var_os("HOME").map(PathBuf::from).unwrap();

        assert_eq!(
            below_home("~/.tool/settings.json"),
            Some(home.join(".tool/settings.json"))
        );
        assert_eq!(below_home("/etc/passwd"), None);
        assert_eq!(below_home("~/../etc/passwd"), None);
        assert_eq!(below_home("~/a/./b"), None);
        assert_eq!(below_home("~/"), None);
        assert_eq!(below_home("~user/x"), None);
    }
}
