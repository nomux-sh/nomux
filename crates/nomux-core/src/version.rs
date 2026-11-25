//! The `--version` output that the nomux binaries share.

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

const EXIT_USAGE: u8 = 2;

/// Makes the version line of the binary that calls the macro: the name, one
/// space, the version.
///
/// Use this macro only in the source of a binary target, because Cargo sets
/// `CARGO_BIN_NAME` only there.
#[macro_export]
macro_rules! version_line {
    () => {
        concat!(env!("CARGO_BIN_NAME"), " ", env!("CARGO_PKG_VERSION"))
    };
}

/// Entry point of a binary that has no commands.
///
/// If the only argument is `--version` or `-V`, this function prints
/// `version_line` on the standard output and returns success. For all other
/// arguments, and for no arguments, it prints a message on the standard error
/// and returns status 2. If the write fails, it returns status 1.
pub fn version_only_main(version_line: &str) -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let written = if is_version_request(&args) {
        writeln!(io::stdout(), "{version_line}").map(|()| ExitCode::SUCCESS)
    } else {
        writeln!(
            io::stderr(),
            "{version_line}: this build has no commands. The only option is --version."
        )
        .map(|()| ExitCode::from(EXIT_USAGE))
    };
    written.unwrap_or(ExitCode::FAILURE)
}

fn is_version_request(args: &[OsString]) -> bool {
    matches!(args, [flag] if flag == "--version" || flag == "-V")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_request(args: &[&str]) -> bool {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        is_version_request(&args)
    }

    #[test]
    fn only_a_single_version_flag_is_a_version_request() {
        assert!(is_request(&["--version"]));
        assert!(is_request(&["-V"]));

        assert!(!is_request(&[]));
        assert!(!is_request(&["attach"]));
        assert!(!is_request(&["--versions"]));
        assert!(!is_request(&["--version", "extra"]));
    }
}
