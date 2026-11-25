//! The rule for the permissions that the user approved: the user cannot
//! approve more than the manifest asks for.

use std::path::Path;

use nomux_extension::Permissions;
use nomux_extension::manifest::PermissionRequest;

/// The variable of a manifest that stands for the directory of a worktree.
const WORKTREE: &str = "${worktree}";

/// Returns the first approved permission that the manifest does not ask
/// for.
pub(super) fn first_not_requested(
    request: &PermissionRequest,
    granted: &Permissions,
) -> Option<String> {
    let fs = granted
        .fs
        .iter()
        .find(|path| !request.fs.iter().any(|asked| fs_matches(asked, path)))
        .map(|path| format!("fs: {path}"));
    let process = || not_listed("process", &granted.process, &request.process);
    let network = || (granted.network && !request.network).then(|| "network".to_owned());
    let scopes = || not_listed("scopes", &granted.scopes, &request.scopes);
    fs.or_else(process).or_else(network).or_else(scopes)
}

fn not_listed(kind: &str, granted: &[String], requested: &[String]) -> Option<String> {
    granted
        .iter()
        .find(|item| !requested.contains(item))
        .map(|item| format!("{kind}: {item}"))
}

/// An approved directory is an absolute path. It is equal to the item of
/// the manifest, or the item starts with `${worktree}` and the directory
/// ends with the rest of the item.
fn fs_matches(requested: &str, granted: &str) -> bool {
    if !Path::new(granted).is_absolute() {
        return false;
    }
    match requested.strip_prefix(WORKTREE) {
        Some(below_worktree) => granted.ends_with(below_worktree),
        None => requested == granted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PermissionRequest {
        let manifest: nomux_extension::manifest::Manifest = r#"
            [extension]
            id = "nomux.test"
            name = "Test"
            version = "1.0.0"
            api = "^0.1"

            [permissions]
            fs = ["/var/cache/test", "${worktree}/.git"]
            process = ["git"]
            scopes = ["sessions:read"]
        "#
        .parse()
        .unwrap();
        manifest.permissions
    }

    fn granted(change: impl FnOnce(&mut Permissions)) -> Option<String> {
        let mut permissions = Permissions::default();
        change(&mut permissions);
        first_not_requested(&request(), &permissions)
    }

    #[test]
    fn a_part_of_the_request_is_accepted() {
        let refused = granted(|permissions| {
            permissions.fs = vec!["/var/cache/test".into(), "/home/user/project/.git".into()];
            permissions.scopes = vec!["sessions:read".into()];
        });

        assert_eq!(refused, None);
    }

    #[test]
    fn each_kind_of_permission_outside_the_request_is_refused() {
        type Change = fn(&mut Permissions);
        let cases: [(Change, &str); 6] = [
            (|p| p.fs = vec!["/etc".into()], "fs: /etc"),
            (
                |p| p.fs = vec!["/home/user/project".into()],
                "fs: /home/user/project",
            ),
            (|p| p.fs = vec!["relative/.git".into()], "fs: relative/.git"),
            (|p| p.process = vec!["sh".into()], "process: sh"),
            (|p| p.network = true, "network"),
            (
                |p| p.scopes = vec!["sessions:write".into()],
                "scopes: sessions:write",
            ),
        ];
        for (change, expected) in cases {
            assert_eq!(granted(change).as_deref(), Some(expected));
        }
    }
}
