//! A package of the test git extension for the end-to-end tests: the binary
//! `nomux-test-git` of this package, with the identifier `nomux.git`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::binary;

/// The file in the home directory with the facts that the test git
/// extension gives: an encoded `WorktreeFactsResponse`.
pub const GIT_FACTS_FILE: &str = "worktree-facts.pb";

/// Makes the package of the test git extension in `parent` and returns its
/// directory.
///
/// # Panics
///
/// Panics if a file of the package cannot be written.
#[must_use]
pub fn git_package(parent: &Path) -> PathBuf {
    let package = parent.join("nomux.git");
    let manifest = "[extension]\nid = \"nomux.git\"\nname = \"Test git\"\nversion = \"1.0.0\"\n\
                    api = \"^0.1.1\"\n\n[daemon]\ncommand = \"bin/git-facts\"\n\n[permissions]\n";
    fs::create_dir_all(package.join("bin")).expect("the package directory can be made");
    fs::write(package.join("extension.toml"), manifest).expect("the manifest can be written");
    fs::copy(
        binary("nomux-test-git"),
        package.join("bin").join("git-facts"),
    )
    .expect("the program of the test git extension can be copied");
    package
}
