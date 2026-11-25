//! Cargo commands on the benchmark workspace.

use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The profiles of the probe binaries: the profile of the core workspace and
/// the same profile optimized for size.
pub(crate) const PROFILES: [&str; 2] = ["release", "small"];

pub(crate) struct Workspace {
    cargo: OsString,
    directory: PathBuf,
    /// The target of the probes. `None` is the host target.
    target: Option<String>,
}

impl Workspace {
    /// On Linux the probes are static musl binaries, as the release binaries
    /// of the holder. Other hosts have no musl target, thus the probes use
    /// the host target.
    pub(crate) fn locate() -> Self {
        let runner_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        let target =
            cfg!(target_os = "linux").then(|| format!("{}-unknown-linux-musl", env::consts::ARCH));
        Self {
            // Cargo sets `CARGO` to its own path when it runs this binary.
            cargo: env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")),
            directory: runner_directory
                .parent()
                .unwrap_or(runner_directory)
                .to_owned(),
            target,
        }
    }

    pub(crate) fn target(&self) -> &str {
        self.target.as_deref().unwrap_or("host")
    }

    /// Builds one probe package and returns the path of its binary.
    pub(crate) fn build(&self, package: &str, profile: &str) -> Result<PathBuf, Box<dyn Error>> {
        let status = self
            .cargo(&[
                "build",
                "--quiet",
                "--profile",
                profile,
                "--package",
                package,
            ])
            .status()?;
        if !status.success() {
            return Err(format!("the build of {package} ({profile}) failed: {status}").into());
        }
        let mut binary = self.directory.join("target");
        binary.extend(&self.target);
        binary.extend([profile, package]);
        Ok(binary)
    }

    /// The number of crates that a probe package adds to a binary: its
    /// normal dependencies for the target, without the benchmark crates.
    pub(crate) fn dependency_count(&self, package: &str) -> Result<usize, Box<dyn Error>> {
        let output = self
            .cargo(&[
                "tree",
                "--edges",
                "normal",
                "--prefix",
                "none",
                "--package",
                package,
            ])
            .stderr(Stdio::inherit())
            .output()?;
        if !output.status.success() {
            return Err(format!("cargo tree failed for {package}: {}", output.status).into());
        }
        let tree = String::from_utf8(output.stdout)?;
        let crates: BTreeSet<&str> = tree
            .lines()
            .filter_map(|line| line.split(' ').next())
            .filter(|name| !name.is_empty() && *name != package && *name != "vt-probe")
            .collect();
        Ok(crates.len())
    }

    fn cargo(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(&self.cargo);
        command.args(arguments).current_dir(&self.directory);
        if let Some(target) = &self.target {
            command.args(["--target", target]);
        }
        command
    }
}
