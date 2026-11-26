//! The local check of the manifest of a package, with the validator of the
//! extension SDK.

use std::fs;
use std::io;
use std::path::Path;

use nomux_client::{ClientError, ExtensionPackage, ExtensionPermissions};
use nomux_extension::manifest::{Manifest, ManifestError};

const MANIFEST_FILE: &str = "extension.toml";

/// The result of the check of one `extension.toml`.
#[derive(Debug)]
pub(crate) enum ManifestCheck {
    Valid(ExtensionPackage),
    NotValid(Vec<ManifestProblem>),
}

/// One reason why a manifest is not valid.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ManifestProblem {
    /// The path of the field, for example `extension.id`. `None` if the
    /// file is not TOML.
    pub(crate) path: Option<String>,
    /// The rule that the field or the file does not obey.
    pub(crate) problem: String,
}

/// Reads and validates the manifest of the package in `directory`.
pub(super) fn check(directory: &Path) -> Result<ManifestCheck, ClientError> {
    let file = directory.join(MANIFEST_FILE);
    let text = fs::read_to_string(&file).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => {
            ClientError::NotFound(format!("the file {} does not exist", file.display()))
        }
        _ => ClientError::Failed(format!(
            "the file {} cannot be read: {error}",
            file.display()
        )),
    })?;
    Ok(validate(&text))
}

/// The package of the manifest `text` of a release archive.
pub(super) fn package_of(text: &str, source: &str) -> Result<ExtensionPackage, ClientError> {
    match validate(text) {
        ManifestCheck::Valid(package) => Ok(package),
        ManifestCheck::NotValid(problems) => {
            let problems: Vec<String> = problems
                .iter()
                .map(|problem| match &problem.path {
                    Some(path) => format!("{path}: {}", problem.problem),
                    None => problem.problem.clone(),
                })
                .collect();
            Err(ClientError::Failed(format!(
                "the {MANIFEST_FILE} of the archive {source} is not valid: {}. Nothing was \
                 installed",
                problems.join("; ")
            )))
        }
    }
}

fn validate(text: &str) -> ManifestCheck {
    match text.parse::<Manifest>() {
        Ok(manifest) => ManifestCheck::Valid(ExtensionPackage {
            id: manifest.id,
            name: manifest.name,
            version: manifest.version.to_string(),
            requested: ExtensionPermissions {
                fs: manifest.permissions.fs,
                process: manifest.permissions.process,
                network: manifest.permissions.network,
                scopes: manifest.permissions.scopes,
            },
        }),
        Err(ManifestError::Syntax(problem)) => ManifestCheck::NotValid(vec![ManifestProblem {
            path: None,
            problem: format!("the file is not TOML: {problem}"),
        }]),
        Err(ManifestError::Fields(fields)) => {
            let problems = fields.into_iter().map(|field| ManifestProblem {
                path: Some(field.path),
                problem: field.problem,
            });
            ManifestCheck::NotValid(problems.collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_field_that_is_not_valid_is_reported_with_its_path() {
        let text = "[extension]\nid = \"Git\"\nname = \"Git\"\nversion = \"1.0.0\"\napi = \"^0.1\"\n\n\
                    [permissions]\nscopes = [\"sessions:read\", \"sessions\"]\n";

        let ManifestCheck::NotValid(problems) = validate(text) else {
            panic!("the manifest is valid");
        };

        let paths: Vec<Option<&str>> = problems
            .iter()
            .map(|problem| problem.path.as_deref())
            .collect();
        assert_eq!(paths, [Some("extension.id"), Some("permissions.scopes[1]")]);
    }

    #[test]
    fn text_that_is_not_toml_is_a_problem_without_a_path() {
        let ManifestCheck::NotValid(problems) = validate("[extension") else {
            panic!("the manifest is valid");
        };

        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].path, None);
    }
}
