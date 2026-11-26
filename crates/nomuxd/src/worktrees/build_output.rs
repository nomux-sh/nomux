//! The rules that decide which ignored directory of a worktree is build
//! output: a directory that a build or an install makes again.
//!
//! The rules are data: [`RULES`], and the rules of the user in
//! `[worktrees.build_output]`. A directory is build output only if git
//! ignores all of it (the git extension gives it as one ignored path) and a
//! rule accepts it. Each other ignored path is a file of the user, for
//! example `.env` or a local database.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use nomux_core::configuration::BuildOutputRule;
use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;

/// The first line of a valid `CACHEDIR.TAG` file.
const CACHE_DIRECTORY_SIGNATURE: &str = "Signature: 8a477f597d28d172789f06886806bc55";

/// The identifier of the rule for a directory with a valid `CACHEDIR.TAG`.
const CACHE_DIRECTORY_RULE: &str = "cache-directory-tag";

/// One rule: directories with one of `names` and one of `markers`.
struct Rule {
    /// The identifier that a client sees, for example `cargo-target`.
    id: &'static str,
    /// The names of the directory.
    names: &'static [&'static str],
    /// The directory is build output if one marker is present. An empty list
    /// accepts the name alone.
    markers: &'static [Marker],
}

/// What makes a directory with a rule name build output.
enum Marker {
    /// A file with this name in the parent directory.
    Beside(&'static str),
    /// A file with this name in the directory.
    Inside(&'static str),
    /// A valid `CACHEDIR.TAG` in the directory.
    CacheDirectoryTag,
}

/// The rules. `dist`, `build` and `out` are not in the list: some
/// repositories keep source files there.
const RULES: &[Rule] = &[
    Rule {
        id: "cargo-target",
        names: &["target"],
        markers: &[Marker::Beside("Cargo.toml"), Marker::CacheDirectoryTag],
    },
    Rule {
        id: "node-modules",
        names: &["node_modules"],
        markers: &[Marker::Beside("package.json")],
    },
    Rule {
        id: "javascript-build-cache",
        names: &[".next", ".nuxt", ".svelte-kit", ".turbo", ".parcel-cache"],
        markers: &[Marker::Beside("package.json")],
    },
    Rule {
        id: "python-virtual-environment",
        names: &[".venv", "venv"],
        markers: &[Marker::Inside("pyvenv.cfg")],
    },
    Rule {
        id: "python-cache",
        names: &["__pycache__", ".mypy_cache", ".ruff_cache", ".pytest_cache"],
        markers: &[],
    },
];

/// The name of the lock file that cargo holds in a profile directory, for
/// example `target/debug/.cargo-lock`, while it builds.
const CARGO_LOCK: &str = ".cargo-lock";

/// The rules of the daemon and the rules of the user.
#[derive(Debug, Default)]
pub(super) struct BuildOutputRules {
    user: Vec<BuildOutputRule>,
}

impl BuildOutputRules {
    pub(super) fn new(user: Vec<BuildOutputRule>) -> Self {
        Self { user }
    }

    /// The identifier of the rule that accepts the ignored path `ignored` of
    /// the worktree `worktree` as build output. `None` for a file of the
    /// user. An ignored directory ends with `/`.
    pub(super) fn rule_of(&self, worktree: &Path, ignored: &str) -> Option<String> {
        let relative = ignored.strip_suffix('/')?;
        let directory = worktree.join(relative);
        let name = directory.file_name()?.to_str()?;
        let accepted = RULES.iter().find(|rule| {
            rule.names.contains(&name)
                && (rule.markers.is_empty()
                    || rule
                        .markers
                        .iter()
                        .any(|marker| marker.is_present(&directory)))
        });
        if let Some(rule) = accepted {
            return Some(rule.id.to_owned());
        }
        if has_cache_directory_tag(&directory) {
            return Some(CACHE_DIRECTORY_RULE.to_owned());
        }
        self.user
            .iter()
            .find(|rule| {
                rule.names.iter().any(|rule_name| rule_name == name)
                    && user_markers_present(rule, &directory)
            })
            .map(|rule| rule.id.clone())
    }
}

fn user_markers_present(rule: &BuildOutputRule, directory: &Path) -> bool {
    let beside = rule.beside.iter().any(|name| {
        directory
            .parent()
            .is_some_and(|parent| is_file(&parent.join(name)))
    });
    let inside = rule
        .inside
        .iter()
        .any(|name| is_file(&directory.join(name)));
    (rule.beside.is_empty() && rule.inside.is_empty()) || beside || inside
}

/// `true` when a build holds a lock in the build output directory
/// `directory`: cargo holds `<profile>/.cargo-lock` and
/// `<target>/<profile>/.cargo-lock` while it builds. The test takes a shared
/// lock that does not wait, and gives it back at once.
pub(super) fn holds_build_lock(directory: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            return false;
        }
        is_locked(&path.join(CARGO_LOCK))
            || fs::read_dir(&path).is_ok_and(|inner| {
                inner
                    .flatten()
                    .any(|entry| is_locked(&entry.path().join(CARGO_LOCK)))
            })
    })
}

fn is_locked(lock_file: &Path) -> bool {
    let Ok(file) = fs::File::open(lock_file) else {
        return false;
    };
    match flock(&file, FlockOperation::NonBlockingLockShared) {
        Ok(()) => false,
        Err(error) => error == Errno::WOULDBLOCK,
    }
}

impl Marker {
    fn is_present(&self, directory: &Path) -> bool {
        match self {
            Self::Beside(name) => directory
                .parent()
                .is_some_and(|parent| is_file(&parent.join(name))),
            Self::Inside(name) => is_file(&directory.join(name)),
            Self::CacheDirectoryTag => has_cache_directory_tag(directory),
        }
    }
}

fn is_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn has_cache_directory_tag(directory: &Path) -> bool {
    let tag = directory.join("CACHEDIR.TAG");
    if !is_file(&tag) {
        return false;
    }
    let Ok(file) = fs::File::open(tag) else {
        return false;
    };
    let mut first_line = String::new();
    // The signature line is short: the read stops after 64 bytes.
    BufReader::new(file.take(64))
        .read_line(&mut first_line)
        .is_ok()
        && first_line.starts_with(CACHE_DIRECTORY_SIGNATURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn a_rule_of_the_user_needs_its_name_and_one_of_its_markers() {
        let directory = tempfile::tempdir().unwrap();
        let worktree = directory.path();
        file(&worktree.join("web/package.json"), "{}");
        fs::create_dir_all(worktree.join("web/dist")).unwrap();
        fs::create_dir_all(worktree.join("docs/dist")).unwrap();
        fs::create_dir_all(worktree.join("cache")).unwrap();
        let rules = BuildOutputRules::new(vec![
            BuildOutputRule {
                id: "javascript-dist".to_owned(),
                names: vec!["dist".to_owned()],
                beside: vec!["package.json".to_owned()],
                inside: Vec::new(),
            },
            BuildOutputRule {
                id: "any-cache".to_owned(),
                names: vec!["cache".to_owned()],
                beside: Vec::new(),
                inside: Vec::new(),
            },
        ]);

        assert_eq!(
            rules.rule_of(worktree, "web/dist/").as_deref(),
            Some("javascript-dist")
        );
        assert_eq!(rules.rule_of(worktree, "docs/dist/"), None);
        assert_eq!(
            rules.rule_of(worktree, "cache/").as_deref(),
            Some("any-cache")
        );
        assert_eq!(rules.rule_of(worktree, "cache"), None);
    }

    #[test]
    fn a_rule_needs_its_name_and_a_marker_and_each_other_ignored_path_is_user_data() {
        let directory = tempfile::tempdir().unwrap();
        let worktree = directory.path();
        file(&worktree.join("Cargo.toml"), "");
        fs::create_dir_all(worktree.join("target")).unwrap();
        file(&worktree.join("web/package.json"), "{}");
        fs::create_dir_all(worktree.join("web/node_modules")).unwrap();
        fs::create_dir_all(worktree.join("web/.next")).unwrap();
        // A `node_modules` without `package.json` beside it is not accepted.
        fs::create_dir_all(worktree.join("other/node_modules")).unwrap();
        file(&worktree.join(".venv/pyvenv.cfg"), "home = /usr\n");
        fs::create_dir_all(worktree.join("venv")).unwrap();
        fs::create_dir_all(worktree.join("src/__pycache__")).unwrap();
        file(
            &worktree.join("cache/CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n# a comment\n",
        );
        file(
            &worktree.join("fake-cache/CACHEDIR.TAG"),
            "Signature: 0000\n",
        );
        fs::create_dir_all(worktree.join("dist")).unwrap();

        let rules: Vec<(&str, Option<String>)> = [
            "target/",
            "web/node_modules/",
            "web/.next/",
            "other/node_modules/",
            ".venv/",
            "venv/",
            "src/__pycache__/",
            "cache/",
            "fake-cache/",
            "dist/",
            ".env",
            "local.db",
        ]
        .into_iter()
        .map(|path| (path, BuildOutputRules::default().rule_of(worktree, path)))
        .collect();

        assert_eq!(
            rules
                .iter()
                .map(|(path, rule)| (*path, rule.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("target/", Some("cargo-target")),
                ("web/node_modules/", Some("node-modules")),
                ("web/.next/", Some("javascript-build-cache")),
                ("other/node_modules/", None),
                (".venv/", Some("python-virtual-environment")),
                ("venv/", None),
                ("src/__pycache__/", Some("python-cache")),
                ("cache/", Some("cache-directory-tag")),
                ("fake-cache/", None),
                ("dist/", None),
                (".env", None),
                ("local.db", None),
            ]
        );
    }
}
