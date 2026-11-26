//! A package of the test adapter for the end-to-end tests: the binary
//! `nomux-test-adapter` of this package, the adapter of `testtool`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::binary;

/// The identifier of the test adapter.
pub const TEST_ADAPTER: &str = "nomux.test-adapter";

/// Makes the package of the test adapter in `parent` and returns its
/// directory. The manifest asks for the scopes of an adapter.
///
/// # Panics
///
/// Panics if a file of the package cannot be written.
#[must_use]
pub fn adapter_package(parent: &Path) -> PathBuf {
    let package = parent.join(TEST_ADAPTER);
    let manifest = format!(
        "[extension]\nid = \"{TEST_ADAPTER}\"\nname = \"Test adapter\"\nversion = \"1.0.0\"\n\
         api = \"^0.1.1\"\n\n[daemon]\ncommand = \"bin/adapter\"\n\n[adapter]\n\
         tools = [\"testtool\"]\n\n[permissions]\nscopes = [\"hooks:read\", \"agent:report\"]\n"
    );
    fs::create_dir_all(package.join("bin")).expect("the package directory can be made");
    fs::write(package.join("extension.toml"), manifest).expect("the manifest can be written");
    fs::copy(
        binary("nomux-test-adapter"),
        package.join("bin").join("adapter"),
    )
    .expect("the program of the test adapter can be copied");
    package
}
