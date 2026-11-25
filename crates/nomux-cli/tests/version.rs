//! Runs the real `nomux` binary without a daemon: the version output and
//! the exit code of a usage error. `nomuxd` and `nomux-holder` have their
//! own tests.

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_nomux");

fn nomux(arguments: &[&str]) -> Output {
    Command::new(BIN)
        .args(arguments)
        .output()
        .expect("the test helper works")
}

#[test]
fn version_flag_prints_name_and_version() {
    let output = nomux(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("nomux ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

/// Exit code 2 means "not found". A wrong command line is an error.
#[test]
fn a_usage_error_has_exit_code_1() {
    for arguments in [&[][..], &["no-such-command"], &["ls", "--json", "--quiet"]] {
        let output = nomux(arguments);
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(message.contains("Usage: nomux"), "{arguments:?}: {message}");
    }
}
