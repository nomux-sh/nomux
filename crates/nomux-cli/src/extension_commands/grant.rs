//! The approval of the permissions of an extension on the command line.
//!
//! One permission is one item of text: `fs=<directory>`,
//! `process=<program>`, `network` or `scope=<scope>`. The `--grant` option
//! reads an item, and each output that lists permissions writes the same
//! form.

use std::str::FromStr;

use nomux_client::{ClientError, ExtensionPackage, ExtensionPermissions};

/// The variable of a manifest that stands for the directory of a worktree.
const WORKTREE: &str = "${worktree}";

const FS: &str = "fs";
const PROCESS: &str = "process";
const NETWORK: &str = "network";
const SCOPE: &str = "scope";

/// One permission that the user approves with `--grant`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Grant {
    Fs(String),
    Process(String),
    Network,
    Scope(String),
}

impl FromStr for Grant {
    type Err = String;

    fn from_str(item: &str) -> Result<Self, Self::Err> {
        let grant = match item.split_once('=') {
            None if item == NETWORK => Some(Self::Network),
            Some((FS, directory)) => Some(Self::Fs(directory.to_owned())),
            Some((PROCESS, program)) => Some(Self::Process(program.to_owned())),
            Some((SCOPE, scope)) => Some(Self::Scope(scope.to_owned())),
            _ => None,
        };
        grant.ok_or_else(|| {
            format!(
                "`{item}` is not a permission: use {FS}=<directory>, {PROCESS}=<program>, \
                 {NETWORK} or {SCOPE}=<scope>"
            )
        })
    }
}

/// The permissions as items, in the sequence `fs`, `process`, `network`,
/// `scope`.
pub(crate) fn permission_items(permissions: &ExtensionPermissions) -> Vec<String> {
    let items = |kind: &'static str, values: &[String]| {
        let items = values.iter().map(move |value| format!("{kind}={value}"));
        items.collect::<Vec<String>>()
    };
    let network = permissions.network.then(|| NETWORK.to_owned());
    [
        items(FS, &permissions.fs),
        items(PROCESS, &permissions.process),
        network.into_iter().collect(),
        items(SCOPE, &permissions.scopes),
    ]
    .concat()
}

/// The permissions that the command line approves. `--grant-all` approves
/// each requested permission that is the same for each use of the
/// extension. A directory below `${worktree}` is not: the user approves a
/// directory for it with `--grant fs=<directory>`.
pub(super) fn approved(
    requested: &ExtensionPermissions,
    all: bool,
    grants: Vec<Grant>,
) -> ExtensionPermissions {
    let mut approved = ExtensionPermissions::default();
    if all {
        approved = requested.clone();
        approved
            .fs
            .retain(|directory| !directory.starts_with(WORKTREE));
    }
    for grant in grants {
        match grant {
            Grant::Fs(directory) => add(&mut approved.fs, directory),
            Grant::Process(program) => add(&mut approved.process, program),
            Grant::Network => approved.network = true,
            Grant::Scope(scope) => add(&mut approved.scopes, scope),
        }
    }
    approved
}

fn add(items: &mut Vec<String>, item: String) {
    if !items.contains(&item) {
        items.push(item);
    }
}

/// The error of an installation without an approval. It shows the
/// permissions that the extension asks for and how to approve them.
pub(super) fn approval_necessary(package: &ExtensionPackage) -> ClientError {
    let mut message = format!(
        "the extension {} {} asks for these permissions:\n",
        package.id, package.version
    );
    for item in permission_items(&package.requested) {
        message.push_str("  ");
        message.push_str(&item);
        message.push('\n');
    }
    message.push_str(
        "Nothing was installed. To install the extension, approve its permissions: add \
         --grant-all, or --grant <permission> for each permission that you approve. For a \
         directory that starts with ${worktree}, give a directory: --grant fs=<directory>",
    );
    ClientError::PermissionDenied(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested() -> ExtensionPermissions {
        ExtensionPermissions {
            fs: vec!["/var/cache/git".to_owned(), "${worktree}/.git".to_owned()],
            process: vec!["git".to_owned()],
            network: true,
            scopes: vec!["sessions:read".to_owned()],
        }
    }

    #[test]
    fn a_grant_is_read_in_the_form_in_which_a_permission_is_shown() {
        let items = permission_items(&requested());

        assert_eq!(
            items,
            [
                "fs=/var/cache/git",
                "fs=${worktree}/.git",
                "process=git",
                "network",
                "scope=sessions:read"
            ]
        );
        let grants: Vec<Grant> = items.iter().map(|item| item.parse().unwrap()).collect();
        assert_eq!(approved(&requested(), false, grants), requested());
    }

    #[test]
    fn text_that_is_not_a_permission_is_refused() {
        for item in ["", "network=yes", "scopes=sessions:read", "git", "fs"] {
            assert!(item.parse::<Grant>().is_err(), "{item}");
        }
    }

    #[test]
    fn grant_all_approves_no_directory_of_a_worktree() {
        let all = approved(&requested(), true, Vec::new());

        assert_eq!(all.fs, ["/var/cache/git"]);
        assert_eq!(all.process, requested().process);
        assert!(all.network);
        assert_eq!(all.scopes, requested().scopes);

        let directory = Grant::Fs("/home/user/project/.git".to_owned());
        let with_directory = approved(
            &requested(),
            true,
            vec![directory, Grant::Process("git".to_owned())],
        );
        assert_eq!(
            with_directory.fs,
            ["/var/cache/git", "/home/user/project/.git"]
        );
        // An item that `--grant-all` already approved is there one time.
        assert_eq!(with_directory.process, ["git"]);
    }

    #[test]
    fn without_grant_all_only_the_named_permissions_are_approved() {
        let one = approved(
            &requested(),
            false,
            vec![Grant::Scope("sessions:read".to_owned())],
        );

        assert_eq!(
            one,
            ExtensionPermissions {
                scopes: vec!["sessions:read".to_owned()],
                ..ExtensionPermissions::default()
            }
        );
    }
}
