//! The `[sandbox]` table of the configuration file: the default profile of
//! a session, the defaults of the agent tools and of the projects, and the
//! profiles of the user. The daemon resolves a profile into the paths of a
//! plan; `docs/sandbox.md` has the rules.
//!
//! ```toml
//! [sandbox]
//! default = "off"
//! strict = false
//!
//! [sandbox.tools]
//! claude = "project"
//!
//! [sandbox.projects]
//! "/home/user/src/api" = "project-offline"
//!
//! [sandbox.profiles.mine]
//! extends = "project"
//! write = ["~/scratch"]
//! deny_read = ["~/work/private"]
//! network = "off"
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use toml_edit::{Item, Table};

/// The settings of the `[sandbox]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxSettings {
    /// The profile of a session that names none and has no other default:
    /// `default`. `None` is no sandbox.
    pub default: Option<String>,
    /// A session whose host cannot apply each feature that its profile
    /// wants does not start: `strict`.
    pub strict: bool,
    /// The profile of a session whose command is an agent tool, by the
    /// name of the tool: `[sandbox.tools]`.
    pub tools: BTreeMap<String, String>,
    /// The profile of a session in a project directory or below it, by the
    /// absolute path of the directory: `[sandbox.projects]`.
    pub projects: BTreeMap<PathBuf, String>,
    /// The profiles of the user, by their name: `[sandbox.profiles]`.
    pub profiles: BTreeMap<String, UserProfile>,
}

/// One profile of the user: a built-in profile with more rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserProfile {
    /// The built-in profile that it extends: `extends`.
    pub extends: String,
    /// More paths that the command can read: `read`. `~` is the home
    /// directory.
    pub read: Vec<String>,
    /// More paths that the command can change: `write`.
    pub write: Vec<String>,
    /// More paths that the command cannot read: `deny_read`.
    pub deny_read: Vec<String>,
    /// `Some(true)` for `network = "off"`, `Some(false)` for `"open"`.
    /// `None` keeps the network of the extended profile.
    pub network_off: Option<bool>,
}

/// The name of a profile: 1 to 64 characters from `a-z`, `0-9` and `-`.
#[must_use]
pub fn is_profile_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Reads the `[sandbox]` table. The error is the key and the problem.
pub(super) fn parse(table: &Table) -> Result<SandboxSettings, (String, String)> {
    let mut settings = SandboxSettings::default();
    for (key, item) in table {
        match key {
            "default" => settings.default = Some(profile_name(key, item)?),
            "strict" => {
                settings.strict = item
                    .as_bool()
                    .ok_or_else(|| (key.to_owned(), "must be true or false".to_owned()))?;
            }
            "tools" => {
                for (tool, item) in sub_table(key, item)? {
                    let profile = profile_name(&format!("tools.{tool}"), item)?;
                    settings.tools.insert(tool.to_owned(), profile);
                }
            }
            "projects" => {
                for (directory, item) in sub_table(key, item)? {
                    let path = PathBuf::from(directory);
                    if !path.is_absolute() {
                        return Err((
                            format!("projects.{directory}"),
                            "must be an absolute path".to_owned(),
                        ));
                    }
                    let profile = profile_name(&format!("projects.{directory}"), item)?;
                    settings.projects.insert(path, profile);
                }
            }
            "profiles" => {
                for (name, item) in sub_table(key, item)? {
                    let key = format!("profiles.{name}");
                    if !is_profile_name(name) {
                        return Err((key, NAME_RULE.to_owned()));
                    }
                    let profile = user_profile(&key, item)?;
                    settings.profiles.insert(name.to_owned(), profile);
                }
            }
            _ => return Err((key.to_owned(), "is not a key of [sandbox]".to_owned())),
        }
    }
    Ok(settings)
}

const NAME_RULE: &str = "must be 1 to 64 characters from a-z, 0-9 and -";

fn sub_table<'a>(key: &str, item: &'a Item) -> Result<&'a Table, (String, String)> {
    item.as_table()
        .ok_or_else(|| (key.to_owned(), "must be a table".to_owned()))
}

fn profile_name(key: &str, item: &Item) -> Result<String, (String, String)> {
    item.as_str()
        .filter(|name| is_profile_name(name))
        .map(str::to_owned)
        .ok_or_else(|| {
            (
                key.to_owned(),
                format!("must be a profile name: {NAME_RULE}"),
            )
        })
}

fn user_profile(key: &str, item: &Item) -> Result<UserProfile, (String, String)> {
    let table = sub_table(key, item)?;
    let mut profile = UserProfile::default();
    for (field, item) in table {
        let field_key = format!("{key}.{field}");
        match field {
            "extends" => profile.extends = profile_name(&field_key, item)?,
            "read" => profile.read = paths(&field_key, item)?,
            "write" => profile.write = paths(&field_key, item)?,
            "deny_read" => profile.deny_read = paths(&field_key, item)?,
            "network" => {
                profile.network_off = Some(match item.as_str() {
                    Some("off") => true,
                    Some("open") => false,
                    _ => return Err((field_key, "must be \"off\" or \"open\"".to_owned())),
                });
            }
            _ => return Err((field_key, "is not a key of a profile".to_owned())),
        }
    }
    if profile.extends.is_empty() {
        return Err((
            format!("{key}.extends"),
            "is necessary: the name of a built-in profile".to_owned(),
        ));
    }
    Ok(profile)
}

/// A list of paths: each one absolute, or `~` and a path below the home
/// directory.
fn paths(key: &str, item: &Item) -> Result<Vec<String>, (String, String)> {
    let problem = || {
        (
            key.to_owned(),
            "must be a list of absolute paths or paths that start with ~/".to_owned(),
        )
    };
    item.as_array()
        .ok_or_else(problem)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|path| path.starts_with('/') || path.starts_with("~/"))
                .map(str::to_owned)
                .ok_or_else(problem)
        })
        .collect()
}
