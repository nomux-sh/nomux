//! The text form of the report of an `ext` command.

use std::fmt::Write;

use nomux_client::{Extension, ExtensionPackage, ExtensionPermissions, ExtensionState};

use super::table::{Cell, Table};

use crate::extension_commands::{
    ExtensionReport, ManifestCheck, ManifestProblem, permission_items,
};

/// The mark of an empty cell of a table.
const NONE: &str = "-";

pub(super) fn report(report: &ExtensionReport, width: Option<usize>) -> String {
    match report {
        ExtensionReport::Listed(extensions) if extensions.is_empty() => {
            "no extensions\n".to_owned()
        }
        ExtensionReport::Listed(extensions) => extension_table(extensions, width),
        ExtensionReport::Installed(extension) => installed(extension),
        ExtensionReport::Removed { id } => format!("extension {id} removed\n"),
        ExtensionReport::Enabled(extension) => format!("extension {} enabled\n", extension.id),
        ExtensionReport::Disabled(extension) => format!("extension {} disabled\n", extension.id),
        ExtensionReport::ManifestChecked(ManifestCheck::Valid(package)) => valid_manifest(package),
        ExtensionReport::ManifestChecked(ManifestCheck::NotValid(problems)) => {
            manifest_problems(problems)
        }
        // The payload of a result is not text: `output` writes its bytes.
        ExtensionReport::Called { .. } => String::new(),
    }
}

/// Only what a script needs: identifiers, or nothing when the exit status
/// is the result.
pub(super) fn quiet_report(report: &ExtensionReport) -> String {
    match report {
        ExtensionReport::Listed(extensions) => extensions
            .iter()
            .flat_map(|extension| [extension.id.as_str(), "\n"])
            .collect(),
        _ => String::new(),
    }
}

pub(super) fn state_name(state: ExtensionState) -> &'static str {
    match state {
        ExtensionState::Stopped => "stopped",
        ExtensionState::Starting => "starting",
        ExtensionState::Running => "running",
        ExtensionState::Failed => "failed",
        ExtensionState::Disabled => "disabled",
        ExtensionState::Unknown => "unknown",
    }
}

fn installed(extension: &Extension) -> String {
    format!(
        "extension {} {} installed with {}\n",
        extension.id,
        extension.version,
        permission_list(&extension.granted, "the permissions:")
    )
}

fn valid_manifest(package: &ExtensionPackage) -> String {
    format!(
        "extension.toml is valid: {} {} ({}) asks for {}\n",
        package.id,
        package.version,
        package.name,
        permission_list(&package.requested, "the permissions:")
    )
}

/// `<intro> <item> <item>`, or `no permissions`.
fn permission_list(permissions: &ExtensionPermissions, intro: &str) -> String {
    let items = permission_items(permissions);
    if items.is_empty() {
        return "no permissions".to_owned();
    }
    format!("{intro} {}", items.join(" "))
}

fn manifest_problems(problems: &[ManifestProblem]) -> String {
    let mut text = "extension.toml is not valid:\n".to_owned();
    for problem in problems {
        // A write to a `String` does not fail.
        let _ = match &problem.path {
            Some(path) => writeln!(text, "  `{path}` {}", problem.problem),
            None => writeln!(text, "  {}", problem.problem),
        };
    }
    text
}

/// One line for each extension. The last column says why an extension is
/// disabled or how its process failed.
fn extension_table(extensions: &[Extension], width: Option<usize>) -> String {
    const PERMISSIONS: usize = 4;
    const REASON: usize = 5;
    let mut table = Table::new(&["ID", "NAME", "VERSION", "STATE", "PERMISSIONS", "REASON"])
        .shorten(&[REASON, PERMISSIONS]);
    for extension in extensions {
        table.push(table_row(extension).into_iter().map(Cell::new).collect());
    }
    table.render(width)
}

fn table_row(extension: &Extension) -> [String; 6] {
    let permissions = permission_items(&extension.granted);
    let permissions = if permissions.is_empty() {
        NONE.to_owned()
    } else {
        permissions.join(",")
    };
    [
        extension.id.clone(),
        extension.name.clone(),
        extension.version.clone(),
        state_name(extension.state).to_owned(),
        permissions,
        extension.state_reason.clone().unwrap_or_default(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extension(id: &str, state: ExtensionState, reason: Option<&str>) -> Extension {
        Extension {
            id: id.to_owned(),
            name: "Git".to_owned(),
            version: "1.0.0".to_owned(),
            state,
            state_reason: reason.map(str::to_owned),
            granted: ExtensionPermissions {
                process: vec!["git".to_owned()],
                scopes: vec!["sessions:read".to_owned()],
                ..ExtensionPermissions::default()
            },
        }
    }

    #[test]
    fn the_table_shows_why_an_extension_is_disabled() {
        let mut echo = extension("nomux.echo", ExtensionState::Running, None);
        echo.granted = ExtensionPermissions::default();
        let git = extension(
            "nomux.git",
            ExtensionState::Disabled,
            Some("the extension failed: the process ended: exit status: 7"),
        );

        assert_eq!(
            extension_table(&[echo, git], None),
            "ID          NAME  VERSION  STATE     PERMISSIONS                      REASON\n\
             nomux.echo  Git   1.0.0    running   -\n\
             nomux.git   Git   1.0.0    disabled  process=git,scope=sessions:read  the extension failed: the process ended: exit status: 7\n"
        );
    }

    #[test]
    fn an_installation_shows_the_permissions_that_the_extension_got() {
        let mut git = extension("nomux.git", ExtensionState::Stopped, None);
        assert_eq!(
            installed(&git),
            "extension nomux.git 1.0.0 installed with the permissions: process=git \
             scope=sessions:read\n"
        );

        git.granted = ExtensionPermissions::default();
        assert_eq!(
            installed(&git),
            "extension nomux.git 1.0.0 installed with no permissions\n"
        );
    }

    #[test]
    fn each_problem_of_a_manifest_is_one_line_with_the_path_of_its_field() {
        let problems = [
            ManifestProblem {
                path: Some("extension.id".to_owned()),
                problem: "must have two or more parts".to_owned(),
            },
            ManifestProblem {
                path: None,
                problem: "the file is not TOML: line 1".to_owned(),
            },
        ];

        assert_eq!(
            manifest_problems(&problems),
            "extension.toml is not valid:\n  `extension.id` must have two or more parts\n  the \
             file is not TOML: line 1\n"
        );
    }
}
