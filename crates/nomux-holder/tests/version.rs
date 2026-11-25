//! `nomux-holder --version` has the same source as the other binaries.

use std::process::Command;

const HOLDER: &str = env!("CARGO_BIN_EXE_nomux-holder");

#[test]
fn version_flag_prints_name_and_version() {
    let output = Command::new(HOLDER).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("nomux-holder ", env!("CARGO_PKG_VERSION"), "\n")
    );
}

#[test]
fn no_arguments_is_a_usage_error() {
    let output = Command::new(HOLDER).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
}
