//! The sandbox of a session with the real binaries: what a process of a
//! session under `project` can read, write and reach, the start that fails
//! closed, and the profile after a hibernation and a wake.
//!
//! A host without Landlock skips each test and prints the reason. CI runs
//! on the kernel of the GitHub runner: see `docs/sandbox.md`.

use std::fs;
use std::path::Path;

use nomux_end_to_end::{Sandbox, adapter_package, binary, wait_for};
use serde_json::Value;

/// `true` if the kernel applies Landlock. A host without it starts no
/// sandboxed session: the test of that case is
/// `a_profile_that_the_host_cannot_apply_starts_no_session`.
fn has_landlock(sandbox: &Sandbox) -> bool {
    let status = sandbox.nomux_ok(&["daemon", "status", "--json"]).json();
    let enforced = &status["daemon"]["sandbox"]["enforced"];
    let has_files = enforced
        .as_array()
        .is_some_and(|features| features.iter().any(|feature| feature == "files"));
    let detail = &status["daemon"]["sandbox"]["detail"];
    // CI shows what the kernel of the runner applies.
    eprintln!("sandbox of this host: {detail}");
    if !has_files {
        println!("skipped: this host applies no sandbox");
    }
    has_files
}

fn has_feature(sandbox: &Sandbox, feature: &str) -> bool {
    let status = sandbox.nomux_ok(&["daemon", "status", "--json"]).json();
    status["daemon"]["sandbox"]["enforced"]
        .as_array()
        .is_some_and(|features| features.iter().any(|known| known == feature))
}

/// A home directory with a secret, a project and a directory beside it.
fn home_with_project() -> Sandbox {
    let sandbox = Sandbox::new();
    let home = sandbox.home();
    fs::create_dir_all(home.join(".ssh")).unwrap();
    fs::write(home.join(".ssh/id_test"), "secret key").unwrap();
    fs::create_dir_all(home.join("project/.git")).unwrap();
    fs::create_dir_all(home.join("other")).unwrap();
    // The daemon starts now: its daemon status has the sandbox.
    let _ = sandbox.sessions();
    sandbox
}

/// Runs `script` with `sh` in a session under `profile` in the project, and
/// gives its output when the script wrote `done`.
fn run_in_session(sandbox: &Sandbox, profile: &str, script: &str) -> String {
    let project = sandbox.home().join("project");
    let marker = project.join("done");
    let script = format!(
        "{{ {script}; }} > {out} 2>&1; touch {done}; exec sleep 300",
        out = project.join("out").display(),
        done = marker.display(),
    );
    let _ = sandbox.nomux_ok(&[
        "new",
        "--sandbox",
        profile,
        "--cwd",
        project.to_str().unwrap(),
        "--",
        "sh",
        "-c",
        &script,
    ]);
    wait_for("the script of the session ends", || {
        marker.exists().then_some(())
    });
    fs::read_to_string(project.join("out")).unwrap()
}

#[test]
fn a_session_under_project_reads_no_secret_and_writes_only_in_the_project() {
    let sandbox = home_with_project();
    if !has_landlock(&sandbox) {
        return;
    }
    let home = sandbox.home().display().to_string();
    let output = run_in_session(
        &sandbox,
        "project",
        &format!(
            "cat {home}/.ssh/id_test || echo read-denied; \
             echo x > {home}/other/file || echo write-outside-denied; \
             echo inside > note && cat note; \
             echo tmp > \"$TMPDIR/file\" && echo tmp-works; \
             cat /etc/hostname > /dev/null && echo system-readable"
        ),
    );

    assert!(!output.contains("secret key"), "{output}");
    for expected in [
        "read-denied",
        "write-outside-denied",
        "inside",
        "tmp-works",
        "system-readable",
    ] {
        assert!(output.contains(expected), "{expected}: {output}");
    }
    assert!(!sandbox.home().join("other/file").exists());

    let session = &sandbox.sessions()[0];
    assert_eq!(session["sandbox"]["profile"], "project", "{session}");
    assert!(
        session["sandbox"]["enforced"]
            .as_array()
            .unwrap()
            .contains(&Value::from("files")),
        "{session}"
    );
    let table = sandbox.nomux_ok(&["ls"]).stdout;
    assert!(
        table.contains("SANDBOX") && table.contains("project"),
        "{table}"
    );
}

#[test]
fn a_build_tool_runs_in_the_project_and_reads_the_system_paths() {
    let sandbox = home_with_project();
    if !has_landlock(&sandbox) {
        return;
    }
    let Some(compiler) = ["/usr/bin/cc", "/usr/bin/gcc", "/usr/bin/clang"]
        .into_iter()
        .find(|path| Path::new(path).exists())
    else {
        println!("skipped: this host has no C compiler in /usr/bin");
        return;
    };
    fs::write(
        sandbox.home().join("project/main.c"),
        "#include <stdio.h>\nint main(void) { puts(\"built\"); return 0; }\n",
    )
    .unwrap();

    let output = run_in_session(
        &sandbox,
        "project-offline",
        &format!("{compiler} -o main main.c && ./main"),
    );

    assert!(output.contains("built"), "{output}");
}

#[test]
fn a_session_under_project_offline_opens_no_network_connection() {
    let sandbox = home_with_project();
    if !has_landlock(&sandbox) || !has_feature(&sandbox, "network_off") {
        return;
    }
    let Some(python) = ["/usr/bin/python3"]
        .into_iter()
        .find(|path| Path::new(path).exists())
    else {
        println!("skipped: this host has no /usr/bin/python3");
        return;
    };
    let script = format!(
        "{python} -c 'import socket\ntry:\n socket.socket(socket.AF_INET, socket.SOCK_DGRAM)\n print(\"open\")\nexcept OSError as e:\n print(\"refused\", e.errno)'"
    );

    let offline = run_in_session(&sandbox, "project-offline", &script);

    assert_eq!(offline.trim(), "refused 13");
}

#[test]
fn a_process_of_a_sandboxed_session_controls_no_session_and_reports_its_status() {
    let sandbox = home_with_project();
    if !has_landlock(&sandbox) {
        return;
    }
    let _ = sandbox.nomux_ok(&["new", "-n", "plain", "--", "sleep", "300"]);
    let nomux = binary("nomux");
    let nomux = nomux.display();
    let output = run_in_session(
        &sandbox,
        "project",
        &format!(
            "{nomux} ls; echo ls=$?; \
             {nomux} send plain hello; echo send=$?; \
             NOMUX_SOCK={home}/.nomux/run/nomuxd.sock {nomux} kill plain; echo kill=$?; \
             {nomux} status set working 'from the sandbox'; echo status=$?",
            home = sandbox.home().display()
        ),
    );

    for refused in ["ls=4", "send=4", "kill=4"] {
        assert!(output.contains(refused), "{refused}: {output}");
    }
    assert!(output.contains("status=0"), "{output}");
    let plain = sandbox
        .sessions()
        .into_iter()
        .find(|session| session["name"] == "plain")
        .unwrap();
    assert_eq!(plain["state"], "running");
    let tree = sandbox.nomux_ok(&["status", "--json"]).stdout;
    assert!(tree.contains("from the sandbox"), "{tree}");
}

#[test]
fn a_profile_that_does_not_exist_or_cannot_apply_starts_no_session() {
    let mut sandbox = home_with_project();
    let project = sandbox.home().join("project");
    let cwd = project.to_str().unwrap().to_owned();

    let unknown = sandbox.nomux(&["new", "--sandbox", "nosuch", "--cwd", &cwd, "--", "true"]);
    assert_eq!(unknown.code, 1, "{unknown:?}");
    assert!(unknown.stderr.contains("does not exist"), "{unknown:?}");

    // A host without the feature that a profile requires: a strict profile
    // requires each feature, and an empty kernel interface has none. The
    // test of a host without Landlock is the same refusal for `files`.
    if has_landlock(&sandbox) && has_feature(&sandbox, "unix_sockets") {
        println!("this kernel applies each feature: the refusal is tested without Landlock only");
    } else {
        sandbox.write_configuration("[sandbox]\nstrict = true\n");
        let _ = sandbox.nomux_ok(&["daemon", "stop"]);
        let refused = sandbox.nomux(&["new", "--sandbox", "project", "--cwd", &cwd, "--", "true"]);
        assert_eq!(refused.code, 1, "{refused:?}");
        assert!(refused.stderr.contains("cannot apply"), "{refused:?}");
    }
    assert_eq!(sandbox.sessions(), Vec::<Value>::new());
}

#[test]
fn a_holder_that_does_not_know_the_sandbox_starts_no_command() {
    let sandbox = home_with_project();
    if !has_landlock(&sandbox) {
        return;
    }
    let _ = sandbox.nomux_ok(&["daemon", "stop"]);
    // An installation whose holder is of an earlier version: it refuses each
    // option that it does not know with its usage error, as the holder of
    // nomux 0.5.0 does with `--sandbox-plan`.
    let bin = sandbox.home().join("old-holder");
    fs::create_dir_all(&bin).unwrap();
    for name in ["nomux", "nomuxd"] {
        fs::copy(binary(name), bin.join(name)).unwrap();
    }
    let holder = bin.join("nomux-holder");
    fs::write(
        &holder,
        format!(
            "#!/bin/sh\ncase \"$*\" in *--sandbox-plan*) echo 'nomux-holder: unknown option --sandbox-plan' >&2; exit 2;; esac\nexec {} \"$@\"\n",
            binary("nomux-holder").display()
        ),
    )
    .unwrap();
    make_executable(&holder);
    let project = sandbox.home().join("project");
    let nomux = bin.join("nomux");
    let output = sandbox
        .command_of(
            &nomux,
            &[
                "new",
                "--sandbox",
                "project",
                "--cwd",
                project.to_str().unwrap(),
                "--",
                "touch",
                "ran",
            ],
        )
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("older than the sandbox"), "{stderr}");
    assert!(!project.join("ran").exists());
    let _ = sandbox.command_of(&nomux, &["daemon", "stop"]).output();
}

fn make_executable(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn a_hibernated_session_wakes_with_its_profile() {
    let mut sandbox = home_with_project();
    if !has_landlock(&sandbox) {
        return;
    }
    let bin = sandbox.home().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let nomux = binary("nomux");
    let tool = bin.join("testtool");
    fs::write(
        &tool,
        format!(
            r#"#!/bin/sh
if [ "$1" = --resume ]; then name=$2; else name=$1; fi
cat "$HOME/.ssh/id_test" > "$HOME/project/$name.$#.read" 2>&1
sleep 0.5
printf '{{"session":"%s","resume":"testtool --resume %s"}}' "$name" "$name" | {nomux} hook testtool prompt
printf '{{}}' | {nomux} hook testtool stop
exec cat
"#,
            nomux = nomux.display()
        ),
    )
    .unwrap();
    make_executable(&tool);
    sandbox.set_variable("PATH", format!("{}:/usr/bin:/bin", bin.display()));
    // The daemon gets the new `PATH` at its next start.
    let _ = sandbox.nomux_ok(&["daemon", "stop"]);
    let package = adapter_package(&sandbox.home().join("packages"));
    let _ = sandbox.nomux_ok(&["ext", "install", package.to_str().unwrap(), "--grant-all"]);
    let project = sandbox.home().join("project");

    let created = sandbox
        .nomux_ok(&[
            "new",
            "--json",
            "--sandbox",
            "project",
            "--cwd",
            project.to_str().unwrap(),
            "--",
            "testtool",
            "alpha",
        ])
        .json();
    let id = created["session"]["id"].as_str().unwrap().to_owned();
    wait_for("the agent reports that it is done", || {
        let explained = sandbox
            .nomux_ok(&["agent", "explain", &id, "--json"])
            .json();
        (explained["explain"]["run"]["status"]["state"] == "done").then_some(())
    });
    let _ = sandbox.nomux_ok(&["hibernate", &id]);
    let _ = sandbox.nomux_ok(&["wake", &id]);
    let resumed_read = project.join("alpha.2.read");
    wait_for("the resumed agent tried the secret", || {
        fs::read_to_string(&resumed_read)
            .ok()
            .filter(|text| !text.is_empty())
    });

    let first = fs::read_to_string(project.join("alpha.1.read")).unwrap();
    let resumed = fs::read_to_string(&resumed_read).unwrap();
    for text in [&first, &resumed] {
        assert!(text.contains("Permission denied"), "{text}");
    }
    let session = sandbox
        .sessions()
        .into_iter()
        .find(|session| session["id"] == id)
        .unwrap();
    assert_eq!(session["sandbox"]["profile"], "project", "{session}");
}

#[test]
fn a_session_with_a_memory_limit_gets_the_limit_and_the_sandbox() {
    let mut sandbox = home_with_project();
    if !has_landlock(&sandbox) {
        return;
    }
    sandbox.write_configuration("[sessions]\nmemory_limit = \"512M\"\n");
    let _ = sandbox.nomux_ok(&["daemon", "stop"]);
    let _ = sandbox.sessions();
    let status = sandbox.nomux_ok(&["daemon", "status", "--json"]).json();
    if status["daemon"]["memory_limit"]["applied"] != true {
        println!(
            "skipped: this host applies no memory limit: {}",
            status["daemon"]["memory_limit"]["detail"]
        );
        return;
    }
    let home = sandbox.home().display().to_string();
    // The launcher of the limit runs before the sandbox: the command is in
    // the scope of the limit, and the limits apply to it.
    let output = run_in_session(
        &sandbox,
        "project",
        &format!(
            "cat /proc/self/cgroup; grep NoNewPrivs /proc/self/status; \
             cat {home}/.ssh/id_test || echo read-denied"
        ),
    );

    assert!(output.contains(".scope"), "{output}");
    assert!(output.contains("NoNewPrivs:\t1"), "{output}");
    assert!(
        output.contains("read-denied") && !output.contains("secret key"),
        "{output}"
    );
    assert!(
        !fs::read_dir(sandbox.runtime_directory())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("plan")),
        "a plan file stayed"
    );
}
