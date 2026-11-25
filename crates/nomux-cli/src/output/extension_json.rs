//! The JSON form of the report of an `ext` command. `docs/cli.md` has the
//! schema.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nomux_client::{Extension, ExtensionPackage, ExtensionPermissions};
use serde::Serialize;

use super::extension_human::state_name;
use crate::extension_commands::{ExtensionReport, ManifestCheck, ManifestProblem};

/// Each variant is one top-level field of the document.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExtensionBody<'a> {
    Extensions(Vec<ExtensionDocument<'a>>),
    Extension(ExtensionDocument<'a>),
    RemovedExtension(&'a str),
    Call(CallDocument<'a>),
    Manifest(ManifestDocument<'a>),
}

impl<'a> From<&'a ExtensionReport> for ExtensionBody<'a> {
    fn from(report: &'a ExtensionReport) -> Self {
        match report {
            ExtensionReport::Listed(extensions) => {
                Self::Extensions(extensions.iter().map(ExtensionDocument::from).collect())
            }
            ExtensionReport::Installed(extension)
            | ExtensionReport::Enabled(extension)
            | ExtensionReport::Disabled(extension) => Self::Extension(extension.into()),
            ExtensionReport::Removed { id } => Self::RemovedExtension(id),
            ExtensionReport::Called {
                id,
                method,
                payload,
            } => Self::Call(CallDocument {
                extension: id,
                method,
                payload_base64: STANDARD.encode(payload),
            }),
            ExtensionReport::ManifestChecked(check) => Self::Manifest(check.into()),
        }
    }
}

#[derive(Serialize)]
pub(super) struct ExtensionDocument<'a> {
    id: &'a str,
    name: &'a str,
    version: &'a str,
    state: &'static str,
    reason: Option<&'a str>,
    permissions: PermissionsDocument<'a>,
}

impl<'a> From<&'a Extension> for ExtensionDocument<'a> {
    fn from(extension: &'a Extension) -> Self {
        Self {
            id: &extension.id,
            name: &extension.name,
            version: &extension.version,
            state: state_name(extension.state),
            reason: extension.state_reason.as_deref(),
            permissions: (&extension.granted).into(),
        }
    }
}

#[derive(Serialize)]
struct PermissionsDocument<'a> {
    fs: &'a [String],
    process: &'a [String],
    network: bool,
    scopes: &'a [String],
}

impl<'a> From<&'a ExtensionPermissions> for PermissionsDocument<'a> {
    fn from(permissions: &'a ExtensionPermissions) -> Self {
        Self {
            fs: &permissions.fs,
            process: &permissions.process,
            network: permissions.network,
            scopes: &permissions.scopes,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CallDocument<'a> {
    extension: &'a str,
    method: &'a str,
    payload_base64: String,
}

/// A valid manifest has its identity and the permissions that it asks for.
/// A manifest that is not valid has its problems.
#[derive(Serialize)]
pub(super) struct ManifestDocument<'a> {
    valid: bool,
    #[serde(flatten)]
    package: Option<PackageDocument<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    problems: Option<Vec<ProblemDocument<'a>>>,
}

#[derive(Serialize)]
struct PackageDocument<'a> {
    id: &'a str,
    name: &'a str,
    version: &'a str,
    permissions: PermissionsDocument<'a>,
}

impl<'a> From<&'a ExtensionPackage> for PackageDocument<'a> {
    fn from(package: &'a ExtensionPackage) -> Self {
        Self {
            id: &package.id,
            name: &package.name,
            version: &package.version,
            permissions: (&package.requested).into(),
        }
    }
}

#[derive(Serialize)]
struct ProblemDocument<'a> {
    path: Option<&'a str>,
    problem: &'a str,
}

impl<'a> From<&'a ManifestProblem> for ProblemDocument<'a> {
    fn from(problem: &'a ManifestProblem) -> Self {
        Self {
            path: problem.path.as_deref(),
            problem: &problem.problem,
        }
    }
}

impl<'a> From<&'a ManifestCheck> for ManifestDocument<'a> {
    fn from(check: &'a ManifestCheck) -> Self {
        match check {
            ManifestCheck::Valid(package) => Self {
                valid: true,
                package: Some(package.into()),
                problems: None,
            },
            ManifestCheck::NotValid(problems) => Self {
                valid: false,
                package: None,
                problems: Some(problems.iter().map(ProblemDocument::from).collect()),
            },
        }
    }
}

/// The documents are a contract with scripts. Each test has the full text,
/// thus a changed field name or a removed field fails it.
#[cfg(test)]
mod tests {
    use nomux_client::ExtensionState;

    use crate::extension_commands::ExtensionReport;
    use crate::output::json::report;
    use crate::report::Report;

    use super::*;

    fn document(extension_report: ExtensionReport) -> String {
        report(&Report::Extension(extension_report))
    }

    fn permissions() -> ExtensionPermissions {
        ExtensionPermissions {
            fs: vec!["/var/cache/git".to_owned()],
            process: vec!["git".to_owned()],
            network: true,
            scopes: vec!["sessions:read".to_owned()],
        }
    }

    fn extension() -> Extension {
        Extension {
            id: "nomux.git".to_owned(),
            name: "Git".to_owned(),
            version: "1.0.0".to_owned(),
            state: ExtensionState::Disabled,
            state_reason: Some("the user disabled the extension".to_owned()),
            granted: permissions(),
        }
    }

    const EXTENSION: &str = "{\"id\":\"nomux.git\",\"name\":\"Git\",\"version\":\"1.0.0\",\
        \"state\":\"disabled\",\"reason\":\"the user disabled the extension\",\
        \"permissions\":{\"fs\":[\"/var/cache/git\"],\"process\":[\"git\"],\"network\":true,\
        \"scopes\":[\"sessions:read\"]}}";

    #[test]
    fn extension_documents() {
        assert_eq!(
            document(ExtensionReport::Listed(vec![extension()])),
            format!("{{\"schema_version\":1,\"extensions\":[{EXTENSION}]}}\n")
        );
        assert_eq!(
            document(ExtensionReport::Listed(Vec::new())),
            "{\"schema_version\":1,\"extensions\":[]}\n"
        );
        let one = format!("{{\"schema_version\":1,\"extension\":{EXTENSION}}}\n");
        assert_eq!(document(ExtensionReport::Installed(extension())), one);
        assert_eq!(document(ExtensionReport::Enabled(extension())), one);
        assert_eq!(document(ExtensionReport::Disabled(extension())), one);

        let mut running = extension();
        running.state = ExtensionState::Running;
        running.state_reason = None;
        running.granted = ExtensionPermissions::default();
        assert_eq!(
            document(ExtensionReport::Installed(running)),
            "{\"schema_version\":1,\"extension\":{\"id\":\"nomux.git\",\"name\":\"Git\",\
             \"version\":\"1.0.0\",\"state\":\"running\",\"reason\":null,\"permissions\":\
             {\"fs\":[],\"process\":[],\"network\":false,\"scopes\":[]}}}\n"
        );
        assert_eq!(
            document(ExtensionReport::Removed {
                id: "nomux.git".to_owned()
            }),
            "{\"schema_version\":1,\"removed_extension\":\"nomux.git\"}\n"
        );
    }

    #[test]
    fn the_payload_of_a_result_is_base64_because_it_is_not_text() {
        let called = ExtensionReport::Called {
            id: "nomux.git".to_owned(),
            method: "status".to_owned(),
            payload: vec![0x0a, 0x02, 0xff, 0xfe],
        };

        assert_eq!(
            document(called),
            "{\"schema_version\":1,\"call\":{\"extension\":\"nomux.git\",\"method\":\"status\",\
             \"payload_base64\":\"CgL//g==\"}}\n"
        );
    }

    #[test]
    fn manifest_documents() {
        let valid = ManifestCheck::Valid(ExtensionPackage {
            id: "nomux.git".to_owned(),
            name: "Git".to_owned(),
            version: "1.0.0".to_owned(),
            requested: ExtensionPermissions {
                scopes: vec!["sessions:read".to_owned()],
                ..ExtensionPermissions::default()
            },
        });
        assert_eq!(
            document(ExtensionReport::ManifestChecked(valid)),
            "{\"schema_version\":1,\"manifest\":{\"valid\":true,\"id\":\"nomux.git\",\
             \"name\":\"Git\",\"version\":\"1.0.0\",\"permissions\":{\"fs\":[],\"process\":[],\
             \"network\":false,\"scopes\":[\"sessions:read\"]}}}\n"
        );

        let not_valid = ManifestCheck::NotValid(vec![
            ManifestProblem {
                path: Some("extension.id".to_owned()),
                problem: "must have two or more parts".to_owned(),
            },
            ManifestProblem {
                path: None,
                problem: "the file is not TOML".to_owned(),
            },
        ]);
        assert_eq!(
            document(ExtensionReport::ManifestChecked(not_valid)),
            "{\"schema_version\":1,\"manifest\":{\"valid\":false,\"problems\":[{\"path\":\
             \"extension.id\",\"problem\":\"must have two or more parts\"},{\"path\":null,\
             \"problem\":\"the file is not TOML\"}]}}\n"
        );
    }
}
