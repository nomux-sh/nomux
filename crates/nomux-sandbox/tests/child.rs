//! The limits that the parent builds and the child applies between `fork`
//! and `exec`, with real commands. A host without Landlock skips the test
//! with the reason.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::Path;
use std::process::Command;

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxFeature, SandboxPlan};

const SYSTEM: [&str; 5] = ["/usr", "/bin", "/lib", "/lib64", "/etc"];

fn has_landlock() -> bool {
    let support = nomux_sandbox::probe();
    if support.landlock_abi == 0 {
        println!(
            "skipped: this kernel applies no landlock: {}",
            support.detail()
        );
        return false;
    }
    true
}

fn plan(write: &Path, network_off: bool) -> SandboxPlan {
    SandboxPlan {
        profile: "test".to_owned(),
        read: SYSTEM.iter().map(Into::into).collect(),
        write: vec![write.to_owned()],
        network_off,
        require: FeatureSet::EMPTY.with(SandboxFeature::Files),
        ..SandboxPlan::default()
    }
}

/// Runs `script` with `bash` under `plan` and gives its standard output.
fn run(plan: &SandboxPlan, script: &str) -> String {
    let prepared = nomux_sandbox::prepare(plan).expect("the plan can be applied");
    let mut command = Command::new("/bin/bash");
    command.args(["-c", script]);
    prepared.install(&mut command);
    let output = command.output().expect("the command starts");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_child_writes_only_where_the_plan_permits_and_reads_no_other_file() {
    if !has_landlock() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let inside = directory.path().join("inside");
    let outside = directory.path().join("outside");
    fs::create_dir_all(&inside).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret"), "s").unwrap();
    let script = format!(
        "echo a > {inside}/new && echo write-inside; \
         echo b > {outside}/new || echo write-outside-denied; \
         cat {outside}/secret || echo read-outside-denied",
        inside = inside.display(),
        outside = outside.display()
    );

    let output = run(&plan(&inside, false), &script);

    assert_eq!(
        output,
        "write-inside\nwrite-outside-denied\nread-outside-denied\n"
    );
    assert!(!outside.join("new").exists());
    // The parent has no limit: it reads and writes each file.
    fs::write(outside.join("parent"), "p").unwrap();
}

#[test]
fn without_network_the_child_cannot_open_a_tcp_connection() {
    if !has_landlock() {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    // Port 1 refuses on loopback: the error tells the denial from the
    // refusal.
    let script = "(exec 3<>/dev/tcp/127.0.0.1/1) 2>&1 | grep -q 'Permission denied' \
                  && echo denied || echo refused";

    let open = run(&plan(directory.path(), false), script);
    let closed = run(&plan(directory.path(), true), script);

    assert_eq!(open, "refused\n");
    assert_eq!(closed, "denied\n");
}
