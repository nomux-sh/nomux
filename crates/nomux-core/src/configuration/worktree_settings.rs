//! The `[worktrees]` table: the rules of a clean of the worktrees that the
//! daemon runs without a command, the stale time, the grace time of the
//! trash place, the lifetime of a size, the base branch, the levels of the
//! free space (`[worktrees.disk]`) and the build-output rules of the user
//! (`[worktrees.build_output]`).

use std::path::{Component, Path};
use std::str::FromStr;
use std::time::Duration;

use toml_edit::Item;

use super::{ConfigurationError, ConfigurationFile, size_text};

const WORKTREES: &str = "worktrees";
const AUTO: &str = "auto";
const STALE_AFTER: &str = "stale_after";
const GRACE: &str = "grace";
const SIZE_TTL: &str = "size_ttl";
const BASE: &str = "base";
const DISK: &str = "disk";
const WARN_FREE: &str = "warn_free";
const CRITICAL_FREE: &str = "critical_free";
const BUILD_OUTPUT: &str = "build_output";
const EXTRA: &str = "extra";

/// A rule of `auto`: a clean that the daemon runs without a command of the
/// user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoClean {
    /// `build-output-of-finished`: the build output of each finished
    /// worktree.
    BuildOutputOfFinished,
    /// `finished`: each finished worktree to the trash place.
    Finished,
    /// `stale-build-output`: the build output of each stale worktree.
    StaleBuildOutput,
}

impl FromStr for AutoClean {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "build-output-of-finished" => Ok(Self::BuildOutputOfFinished),
            "finished" => Ok(Self::Finished),
            "stale-build-output" => Ok(Self::StaleBuildOutput),
            _ => Err(format!(
                "`{text}` is not a rule: use \"build-output-of-finished\", \"finished\" or \
                 \"stale-build-output\""
            )),
        }
    }
}

/// A build-output rule of the user: a directory with one of `names` is
/// build output if a file of `beside` is in its parent directory or a file
/// of `inside` is in it. Without `beside` and `inside`, the name alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutputRule {
    /// The identifier that a client shows.
    pub id: String,
    /// The names of the directory.
    pub names: Vec<String>,
    /// Files in the parent directory: one of them makes the directory build
    /// output.
    pub beside: Vec<String>,
    /// Files in the directory: one of them makes it build output.
    pub inside: Vec<String>,
}

impl ConfigurationFile {
    /// The rules of `auto` in `[worktrees]`. Returns `None` if the file does
    /// not have the key.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a list of known rules.
    pub fn worktree_auto_cleans(&self) -> Result<Option<Vec<AutoClean>>, ConfigurationError> {
        self.value(WORKTREES, AUTO, |item| {
            strings(item)?.iter().map(|text| text.parse()).collect()
        })
    }

    /// The time without activity after which a worktree is stale:
    /// `stale_after` in `[worktrees]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a length of time.
    pub fn worktree_stale_after(&self) -> Result<Option<Duration>, ConfigurationError> {
        self.duration_value(WORKTREES, STALE_AFTER, "14d")
    }

    /// The time that a worktree stays in the trash place: `grace` in
    /// `[worktrees]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a length of time.
    pub fn worktree_grace(&self) -> Result<Option<Duration>, ConfigurationError> {
        self.duration_value(WORKTREES, GRACE, "24h")
    }

    /// The time after which the size of a worktree is old: `size_ttl` in
    /// `[worktrees]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a length of time.
    pub fn worktree_size_ttl(&self) -> Result<Option<Duration>, ConfigurationError> {
        self.duration_value(WORKTREES, SIZE_TTL, "1h")
    }

    /// The base branch of the merge rules: `base` in `[worktrees]`. An empty
    /// string is the rule of the git extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a string.
    pub fn worktree_base(&self) -> Result<Option<String>, ConfigurationError> {
        self.text_value(WORKTREES, BASE, |text| Ok(text.to_owned()))
    }

    /// The free bytes below which the daemon warns: `warn_free` in
    /// `[worktrees.disk]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not an amount, for example `"10G"`.
    pub fn worktree_warn_free(&self) -> Result<Option<u64>, ConfigurationError> {
        self.disk_value(WARN_FREE)
    }

    /// The free bytes below which the free space is critical:
    /// `critical_free` in `[worktrees.disk]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not an amount, for example `"3G"`.
    pub fn worktree_critical_free(&self) -> Result<Option<u64>, ConfigurationError> {
        self.disk_value(CRITICAL_FREE)
    }

    /// The build-output rules of the user: `extra` in
    /// `[worktrees.build_output]`, a list of tables with `id`, `names`,
    /// `beside` and `inside`.
    ///
    /// # Errors
    ///
    /// Returns an error if a rule has no `id` or no name, or if a name or a
    /// marker is not one part of a path.
    pub fn worktree_build_output_rules(
        &self,
    ) -> Result<Option<Vec<BuildOutputRule>>, ConfigurationError> {
        self.nested_value(BUILD_OUTPUT, EXTRA, |item| {
            let rules = item
                .as_array()
                .ok_or_else(|| "must be a list of tables".to_owned())?;
            rules
                .iter()
                .map(|rule| {
                    let rule = rule
                        .as_inline_table()
                        .ok_or_else(|| "must be a list of tables".to_owned())?;
                    let id = rule
                        .get("id")
                        .and_then(|id| id.as_str())
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| "each rule needs an `id`".to_owned())?;
                    let list = |key: &str| -> Result<Vec<String>, String> {
                        let Some(value) = rule.get(key) else {
                            return Ok(Vec::new());
                        };
                        let names = value
                            .as_array()
                            .ok_or_else(|| format!("`{key}` must be a list of strings"))?
                            .iter()
                            .map(|name| {
                                name.as_str()
                                    .filter(|name| is_one_path_part(name))
                                    .map(ToOwned::to_owned)
                                    .ok_or_else(|| format!("`{key}` must have names without `/`"))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(names)
                    };
                    let names = list("names")?;
                    if names.is_empty() {
                        return Err(format!("the rule `{id}` needs `names`"));
                    }
                    Ok(BuildOutputRule {
                        id: id.to_owned(),
                        names,
                        beside: list("beside")?,
                        inside: list("inside")?,
                    })
                })
                .collect()
        })
    }

    fn disk_value(&self, key: &str) -> Result<Option<u64>, ConfigurationError> {
        self.nested_value(DISK, key, |item| {
            item.as_str().and_then(size_text::parse).ok_or_else(|| {
                "must be a number with the unit K, M or G, for example \"10G\"".to_owned()
            })
        })
    }

    /// The value of `key` of `[worktrees.<table>]`, as `read` gives it.
    fn nested_value<T>(
        &self,
        table: &str,
        key: &str,
        read: impl FnOnce(&Item) -> Result<T, String>,
    ) -> Result<Option<T>, ConfigurationError> {
        let name = format!("{WORKTREES}.{table}");
        let Some(item) = self.value(WORKTREES, table, |item| Ok(item.clone()))? else {
            return Ok(None);
        };
        let Some(item) = item
            .as_table_like()
            .ok_or_else(|| self.invalid(&name, "must be a table"))?
            .get(key)
        else {
            return Ok(None);
        };
        read(item)
            .map(Some)
            .map_err(|problem| self.invalid(&format!("{name}.{key}"), &problem))
    }
}

fn strings(item: &Item) -> Result<Vec<&str>, String> {
    item.as_array()
        .ok_or_else(|| "must be a list of strings".to_owned())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "must be a list of strings".to_owned())
        })
        .collect()
}

fn is_one_path_part(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(text: &str) -> Result<ConfigurationFile, ConfigurationError> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        ConfigurationFile::load(&path)
    }

    #[test]
    fn the_worktree_settings_are_read_and_a_wrong_value_names_its_key() {
        let file = load(
            r#"
[worktrees]
auto = ["build-output-of-finished", "stale-build-output"]
stale_after = "7d"
grace = "2h"
size_ttl = "30m"
base = "origin/develop"
[worktrees.disk]
warn_free = "20G"
critical_free = "5G"
[worktrees.build_output]
extra = [{ id = "dist", names = ["dist"], beside = ["package.json"] }]
"#,
        )
        .unwrap();

        assert_eq!(
            file.worktree_auto_cleans().unwrap(),
            Some(vec![
                AutoClean::BuildOutputOfFinished,
                AutoClean::StaleBuildOutput
            ])
        );
        assert_eq!(
            file.worktree_stale_after().unwrap(),
            Some(Duration::from_hours(7 * 24))
        );
        assert_eq!(
            file.worktree_grace().unwrap(),
            Some(Duration::from_hours(2))
        );
        assert_eq!(
            file.worktree_size_ttl().unwrap(),
            Some(Duration::from_mins(30))
        );
        assert_eq!(
            file.worktree_base().unwrap().as_deref(),
            Some("origin/develop")
        );
        assert_eq!(file.worktree_warn_free().unwrap(), Some(20 << 30));
        assert_eq!(file.worktree_critical_free().unwrap(), Some(5 << 30));
        assert_eq!(
            file.worktree_build_output_rules().unwrap(),
            Some(vec![BuildOutputRule {
                id: "dist".to_owned(),
                names: vec!["dist".to_owned()],
                beside: vec!["package.json".to_owned()],
                inside: Vec::new(),
            }])
        );

        for (text, key) in [
            ("[worktrees]\nauto = [\"everything\"]\n", "worktrees.auto"),
            ("[worktrees]\ngrace = \"1w\"\n", "worktrees.grace"),
            (
                "[worktrees.disk]\nwarn_free = \"10GB\"\n",
                "worktrees.disk.warn_free",
            ),
            (
                "[worktrees.build_output]\nextra = [{ id = \"x\", names = [\"a/b\"] }]\n",
                "worktrees.build_output.extra",
            ),
            (
                "[worktrees.build_output]\nextra = [{ names = [\"dist\"] }]\n",
                "worktrees.build_output.extra",
            ),
        ] {
            let error = load(text).unwrap_err().to_string();
            assert!(error.contains(key), "{text}: {error}");
        }
    }
}
