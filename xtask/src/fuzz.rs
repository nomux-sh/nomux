//! The `fuzz` task: runs one fuzz target of the `fuzz` directory.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

/// The memory limits of a run, in megabytes. A target that uses more memory
/// than the first limit, or asks for more in one allocation than the second
/// limit, has no bound for its input.
const LIBFUZZER_LIMITS: [&str; 2] = ["-rss_limit_mb=1024", "-malloc_limit_mb=64"];

/// Runs `target` for `seconds`. The run starts with the committed inputs in
/// `fuzz/seeds/<target>` and keeps the inputs that it finds in
/// `fuzz/corpus/<target>`, which git ignores.
pub(crate) fn run(root: &Path, target: &str, seconds: &str) -> ExitCode {
    let fuzz = root.join("fuzz");
    let is_target = fuzz
        .join("fuzz_targets")
        .join(target)
        .with_extension("rs")
        .is_file();
    if !is_target || seconds.parse::<u32>().is_err() {
        eprintln!("xtask: `{target}` is not a fuzz target, or `{seconds}` is not a number");
        return ExitCode::from(2);
    }
    let corpus = Path::new("corpus").join(target);
    if let Err(error) = fs::create_dir_all(fuzz.join(&corpus)) {
        eprintln!("xtask: cannot make the corpus directory: {error}");
        return ExitCode::FAILURE;
    }
    let Some(host) = host_triple(&fuzz) else {
        eprintln!("xtask: cannot read the host triple from `rustc -vV`");
        return ExitCode::FAILURE;
    };

    let status = nightly_command("cargo", &fuzz)
        .args(["fuzz", "run", "--target", &host, target])
        .arg(&corpus)
        .arg(Path::new("seeds").join(target))
        .arg("--")
        .arg(format!("-max_total_time={seconds}"))
        .args(LIBFUZZER_LIMITS)
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("xtask: fuzz target `{target}` failed: {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("xtask: cannot start `cargo fuzz`: {error}");
            ExitCode::FAILURE
        }
    }
}

/// A command that runs in `fuzz` with the nightly toolchain of that
/// directory. The rustup proxy selects that toolchain only without the
/// toolchain of the core workspace in the environment.
fn nightly_command(program: &str, fuzz: &Path) -> Command {
    let mut command = Command::new(program);
    command.current_dir(fuzz).env_remove("RUSTUP_TOOLCHAIN");
    command
}

/// The target triple of this host. Without it, `cargo-fuzz` builds for the
/// triple that its own binary was built for, and a downloaded `cargo-fuzz`
/// is a static binary for a different triple.
fn host_triple(fuzz: &Path) -> Option<String> {
    let output = nightly_command("rustc", fuzz).arg("-vV").output().ok()?;
    let version = String::from_utf8(output.stdout).ok()?;
    let host = version.lines().find_map(|line| line.strip_prefix("host: "));
    host.map(str::to_owned)
}
