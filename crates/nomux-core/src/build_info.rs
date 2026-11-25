//! Name and version of a nomux binary, and the M0 entry point.

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

/// Exit status when the version line was written.
const EXIT_OK: u8 = 0;
/// Exit status when a write to the standard streams failed.
const EXIT_IO: u8 = 1;
/// Exit status when the arguments are not `--version` or `-V`.
const EXIT_USAGE: u8 = 2;

/// Name and version of a nomux binary.
///
/// The [`Display`](fmt::Display) form is the line that `--version` prints:
/// the name, one space, the version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    /// Name of the binary, for example `nomuxd`.
    pub name: &'static str,
    /// Version of the package that contains the binary.
    pub version: &'static str,
}

impl fmt::Display for BuildInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, self.version)
    }
}

/// Makes the [`BuildInfo`](crate::BuildInfo) of the binary that calls the macro.
///
/// Cargo sets the two values at compile time. Use this macro only in the
/// source of a binary target, because Cargo sets `CARGO_BIN_NAME` only there.
#[macro_export]
macro_rules! build_info {
    () => {
        $crate::BuildInfo {
            name: env!("CARGO_BIN_NAME"),
            version: env!("CARGO_PKG_VERSION"),
        }
    };
}

/// Entry point of a binary that has no command yet.
///
/// If `args` is exactly `--version` or `-V`, this function prints the version
/// line on the standard output and returns success. For all other arguments,
/// and for no arguments, it prints a message on the standard error and
/// returns status 2. It does nothing else.
///
/// Milestone M1 replaces this function with the argument parser of each binary.
pub fn version_only_main(info: &BuildInfo, args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let status = run_version_only(
        info,
        args,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(status)
}

fn run_version_only(
    info: &BuildInfo,
    args: impl IntoIterator<Item = OsString>,
    out: &mut impl Write,
    err: &mut impl Write,
) -> u8 {
    let mut args = args.into_iter();
    let is_version = match (args.next(), args.next()) {
        (Some(flag), None) => flag == "--version" || flag == "-V",
        _ => false,
    };
    let written = if is_version {
        writeln!(out, "{info}").map(|()| EXIT_OK)
    } else {
        writeln!(
            err,
            "{info}: this build has no commands. The only option is --version."
        )
        .map(|()| EXIT_USAGE)
    };
    written.unwrap_or(EXIT_IO)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: BuildInfo = BuildInfo {
        name: "nomuxd",
        version: "1.2.3",
    };

    fn run(args: &[&str]) -> (u8, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run_version_only(&INFO, args.iter().map(OsString::from), &mut out, &mut err);
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn display_is_name_then_version() {
        assert_eq!(INFO.to_string(), "nomuxd 1.2.3");
    }

    #[test]
    fn version_flags_print_the_version_line() {
        for flag in ["--version", "-V"] {
            assert_eq!(
                run(&[flag]),
                (EXIT_OK, "nomuxd 1.2.3\n".to_owned(), String::new())
            );
        }
    }

    #[test]
    fn other_arguments_are_a_usage_error() {
        for args in [
            &[][..],
            &["attach"],
            &["--version", "extra"],
            &["--versions"],
        ] {
            let (status, out, err) = run(args);
            assert_eq!(status, EXIT_USAGE, "arguments: {args:?}");
            assert_eq!(out, "", "arguments: {args:?}");
            assert!(
                err.starts_with("nomuxd 1.2.3: this build has no commands"),
                "{err}"
            );
        }
    }

    #[test]
    fn write_failure_is_an_io_error() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let status = run_version_only(
            &INFO,
            [OsString::from("--version")],
            &mut Closed,
            &mut Vec::new(),
        );
        assert_eq!(status, EXIT_IO);
    }
}
