//! A package of the test extension for the end-to-end tests.
//!
//! The program of the package is the binary `nomux-test-extension` of the
//! package `nomux-ext-host`, which uses the extension SDK. The name of the
//! program file selects its behaviour: `crates/nomux-ext-host/tests/test_extension`
//! has the list.

use std::fs;
use std::path::{Path, PathBuf};

use crate::binary;

/// Makes the package of the extension `id` in `parent` and returns its
/// directory. `permissions` has the lines of the `[permissions]` table of
/// the manifest.
///
/// # Panics
///
/// Panics if a file of the package cannot be written.
#[must_use]
pub fn extension_package(parent: &Path, id: &str, permissions: &str) -> PathBuf {
    let package = parent.join(id);
    let manifest = format!(
        "[extension]\nid = \"{id}\"\nname = \"Test\"\nversion = \"1.0.0\"\napi = \"^0.1\"\n\n\
         [daemon]\ncommand = \"bin/{id}\"\n\n[permissions]\n{permissions}\n"
    );
    fs::create_dir_all(package.join("bin")).expect("the package directory can be made");
    fs::write(package.join("extension.toml"), manifest).expect("the manifest can be written");
    fs::copy(binary("nomux-test-extension"), package.join("bin").join(id))
        .expect("the program of the test extension can be copied");
    package
}
