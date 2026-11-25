//! Runs the real `nomux-holder` binary and checks the version output.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_nomux-holder");

#[test]
fn version_flag_prints_name_and_version() {
    let output = Command::new(BIN).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("nomux-holder ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn no_arguments_is_a_usage_error() {
    let output = Command::new(BIN).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("this build has no commands")
    );
}
