//! `nomux worktree ls` with the real binaries and the test git extension:
//! the reason without a git extension and with one that is too old, the states and refusals from the git
//! facts and the sessions, the sizes with a hard link, and the proof that
//! the command changes no file of the repository and runs no `git`.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use nomux_end_to_end::{GIT_FACTS_FILE, Sandbox, extension_package, git_package};
use nomux_proto::wire::v1::{WorktreeFacts, WorktreeFactsResponse, WorktreeMergedBy};
use prost::Message;
use serde_json::Value;

const MEBIBYTE: usize = 1 << 20;

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .current_dir(directory)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(arguments)
        .status()
        .expect("git starts");
    assert!(status.success(), "git {arguments:?}");
}

/// A repository `src/acme` with three linked worktrees next to it: a
/// merged one with build output, one with a file of the user, and one whose
/// directory is gone. Returns the directories.
fn repository(sandbox: &Sandbox) -> [PathBuf; 4] {
    let src = sandbox.home().join("src");
    let main = src.join("acme");
    fs::create_dir_all(&main).expect("the directory is made");
    git(&main, &["init", "--quiet", "--initial-branch=main"]);
    fs::write(main.join(".gitignore"), "target/\n.env\n").expect("a file is written");
    fs::write(main.join("Cargo.toml"), "[package]\n").expect("a file is written");
    git(&main, &["add", "--all"]);
    git(&main, &["commit", "--quiet", "--message", "first"]);
    let merged = src.join("acme-merged");
    let user = src.join("acme-user");
    let gone = src.join("acme-gone");
    for (directory, branch) in [(&merged, "merged"), (&user, "user"), (&gone, "gone")] {
        let path = directory.to_str().expect("a UTF-8 path");
        git(&main, &["worktree", "add", "--quiet", "-b", branch, path]);
    }
    fs::remove_dir_all(&gone).expect("the directory is removed");
    let program = merged.join("target/debug/acme");
    fs::create_dir_all(program.parent().expect("a parent")).expect("the directory is made");
    fs::write(&program, vec![7_u8; MEBIBYTE]).expect("a file is written");
    fs::hard_link(&program, merged.join("target/debug/acme-linked")).expect("a link is made");
    fs::write(user.join(".env"), "SECRET=1\n").expect("a file is written");
    [main, merged, user, gone].map(|path| path.canonicalize().unwrap_or(path))
}

/// The facts that the real git extension gives for the repository.
fn facts(directories: &[PathBuf; 4]) -> WorktreeFactsResponse {
    let [main, merged, user, gone] = directories;
    let worktree = |path: &PathBuf, branch: &str| WorktreeFacts {
        path: path.display().to_string(),
        branch: branch.to_owned(),
        head: "8451064c08cc3f4667602ea1139607109cc12da5".to_owned(),
        last_activity_at_ms: 1,
        ..WorktreeFacts::default()
    };
    WorktreeFactsResponse {
        base_branch: "main".to_owned(),
        last_fetch_at_ms: 0,
        worktrees: vec![
            WorktreeFacts {
                is_main: true,
                ..worktree(main, "main")
            },
            WorktreeFacts {
                merged_by: WorktreeMergedBy::Ancestor.into(),
                ignored_paths: vec!["target/".to_owned()],
                ..worktree(merged, "merged")
            },
            WorktreeFacts {
                ignored_paths: vec![".env".to_owned()],
                ..worktree(user, "user")
            },
            WorktreeFacts {
                is_missing: true,
                ..worktree(gone, "gone")
            },
        ],
    }
}

/// Each file and directory below `root` with its size and its modification
/// time, also in the git directory.
fn listing(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut listing = BTreeMap::new();
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).expect("a directory is read") {
            let entry = entry.expect("an entry is read");
            let metadata = entry.metadata().expect("an entry has metadata");
            if metadata.is_dir() {
                directories.push(entry.path());
            }
            let modified = metadata.modified().expect("an entry has a time");
            listing.insert(entry.path(), (metadata.len(), modified));
        }
    }
    listing
}

/// A directory with a program `git` that writes each call to `calls` and
/// fails. Put first in `PATH`, it shows each `git` that a process runs.
fn logging_git(sandbox: &Sandbox) -> (PathBuf, PathBuf) {
    let directory = sandbox.home().join("logging-bin");
    fs::create_dir_all(&directory).expect("the directory is made");
    let calls = sandbox.home().join("git-calls");
    let script = format!("#!/bin/sh\necho \"$*\" >> {}\nexit 1\n", calls.display());
    let program = directory.join("git");
    fs::write(&program, script).expect("the program is written");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).expect("the mode is set");
    (directory, calls)
}

fn worktree<'a>(document: &'a Value, name: &str) -> &'a Value {
    document["worktrees"]["projects"][0]["worktrees"]
        .as_array()
        .expect("a list of worktrees")
        .iter()
        .find(|worktree| {
            worktree["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(name))
        })
        .unwrap_or_else(|| panic!("no worktree {name} in {document}"))
}

#[test]
fn worktree_ls_shows_the_states_sizes_and_refusals_and_changes_no_file_of_the_repository() {
    let mut sandbox = Sandbox::new();
    let directories = repository(&sandbox);
    // The daemon, the extension and the sessions get this `PATH`.
    let (logging_bin, calls) = logging_git(&sandbox);
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut search_path = logging_bin.into_os_string();
    search_path.push(":");
    search_path.push(path);
    sandbox.set_variable("PATH", search_path);
    let [main, _, user, _] = &directories;
    let main_path = main.to_str().expect("a UTF-8 path");
    let _ = sandbox.nomux_ok(&["project", "add", main_path, "--name", "acme"]);

    let without = sandbox
        .nomux_ok(&["worktree", "ls", "acme", "--json"])
        .json();
    let reason = without["worktrees"]["projects"][0]["unavailable_reason"]
        .as_str()
        .expect("a reason without the git extension");
    assert!(reason.contains("`nomux ext install git"), "{reason}");

    // An extension `nomux.git` without the method `worktree_facts`.
    let old = extension_package(&sandbox.home().join("old"), "nomux.git", "");
    let _ = sandbox.nomux_ok(&[
        "ext",
        "install",
        old.to_str().expect("a path"),
        "--grant-all",
    ]);
    let too_old = sandbox
        .nomux_ok(&["worktree", "ls", "acme", "--json"])
        .json();
    let reason = too_old["worktrees"]["projects"][0]["unavailable_reason"]
        .as_str()
        .expect("a reason with an old git extension");
    assert!(reason.contains("is too old"), "{reason}");
    let _ = sandbox.nomux_ok(&["ext", "rm", "nomux.git"]);

    let package = git_package(&sandbox.home().join("packages"));
    let package = package.display().to_string();
    let _ = sandbox.nomux_ok(&["ext", "install", &package, "--grant-all"]);
    fs::write(
        sandbox.home().join(GIT_FACTS_FILE),
        facts(&directories).encode_to_vec(),
    )
    .expect("the facts are written");
    let user_path = user.to_str().expect("a UTF-8 path");
    let session = sandbox
        .nomux_ok(&["new", "--quiet", "--cwd", user_path, "--", "sleep", "60"])
        .stdout
        .trim()
        .to_owned();
    // `project add` asks git for the root of the repository: only the calls
    // of `worktree ls` count.
    let _ = fs::remove_file(&calls);
    let before = listing(&sandbox.home().join("src"));

    let listed = sandbox
        .nomux_ok(&["worktree", "ls", "acme", "--wait", "--json"])
        .json();
    let text = sandbox.nomux_ok(&["worktree", "ls", "--all"]).stdout;

    assert_eq!(listing(&sandbox.home().join("src")), before);
    assert!(
        !calls.exists(),
        "a process ran git: {:?}",
        fs::read_to_string(&calls)
    );
    let merged = worktree(&listed, "acme-merged");
    assert_eq!(merged["state"], "finished");
    assert_eq!(merged["removable"], true);
    let size = &merged["size"];
    let total = size["total_bytes"].as_u64().expect("a measured size");
    // Two names of one file of 1 MiB: counted one time.
    assert!((1 << 20..3 << 20).contains(&total), "{merged}");
    assert_eq!(size["build_outputs"][0]["path"], "target");
    assert_eq!(size["build_outputs"][0]["rule"], "cargo-target");
    assert_eq!(size["is_old"], false);
    let user = worktree(&listed, "acme-user");
    assert_eq!(user["state"], "active");
    assert_eq!(user["sessions"][0], session.as_str());
    assert_eq!(user["refusals"][0], "active-session");
    assert_eq!(user["ignored_user_paths"][0], ".env");
    assert!(
        user["flags"]
            .as_array()
            .expect("flags")
            .contains(&Value::from("ignored-user-files"))
    );
    let gone = worktree(&listed, "acme-gone");
    assert_eq!(gone["refusals"][0], "directory-missing");
    assert_eq!(gone["size"], Value::Null);
    assert_eq!(worktree(&listed, "acme")["refusals"][0], "main-worktree");
    assert!(text.contains("yes: merged"), "{text}");
    assert!(text.contains("no: session running"), "{text}");
    assert!(text.contains("4 worktrees"), "{text}");
}
