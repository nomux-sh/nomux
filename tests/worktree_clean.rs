//! `nomux worktree clean`, `restore`, `trash` and `mark` with the real
//! binaries, real repositories and the test git extension: the cases of the
//! safety table of the design of issue #37 that the daemon decides. The test
//! git extension gives the facts that a test writes and runs the real `git`
//! for each change without checks of its own: what a test shows refused, the
//! daemon refused. The checks of the real git extension are tested in the
//! `extensions` repository with real repositories.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use nomux_client::{CleanAction, CleanOutcome, CleanRequest, Client, ClientKind, PlanStep};
use nomux_end_to_end::{GIT_CHANGES_FILE, GIT_FACTS_FILE, Sandbox, git_package, wait_for};
use nomux_proto::wire::v1::{WorktreeFacts, WorktreeFactsResponse, WorktreeMergedBy};
use prost::Message;
use rustix::fs::{FlockOperation, flock};
use serde_json::Value;

const MEBIBYTE: usize = 1 << 20;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(arguments)
        .output()
        .expect("git starts");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8")
}

/// A Project `acme` in `src/acme` with the test git extension.
struct Project {
    sandbox: Sandbox,
    main: PathBuf,
    facts: Vec<WorktreeFacts>,
}

impl Project {
    fn new(configuration: Option<&str>) -> Self {
        let mut sandbox = Sandbox::new();
        if let Some(text) = configuration {
            sandbox.write_configuration(text);
        }
        let main = sandbox.home().join("src").join("acme");
        fs::create_dir_all(&main).expect("the directory is made");
        git(&main, &["init", "--quiet", "--initial-branch=main"]);
        fs::write(main.join(".gitignore"), "target/\n.env\n").expect("a file is written");
        fs::write(main.join("Cargo.toml"), "[package]\n").expect("a file is written");
        git(&main, &["add", "--all"]);
        git(&main, &["commit", "--quiet", "--message", "first"]);
        let main = main.canonicalize().expect("a real path");
        let main_path = main.to_str().expect("UTF-8");
        let _ = sandbox.nomux_ok(&["project", "add", main_path, "--name", "acme"]);
        let package = git_package(&sandbox.home().join("packages"));
        let package = package.display().to_string();
        let _ = sandbox.nomux_ok(&["ext", "install", &package, "--grant-all"]);
        let facts = vec![WorktreeFacts {
            is_main: true,
            ..fact(&main, "main")
        }];
        let project = Self {
            sandbox,
            main,
            facts,
        };
        project.write_facts();
        project
    }

    /// Adds the linked worktree `name` next to the main worktree on the new
    /// branch `name`, with a build output directory `target` of 1 MiB, and
    /// returns its directory. `change` sets the facts.
    fn add(&mut self, name: &str, change: impl FnOnce(&mut WorktreeFacts)) -> PathBuf {
        let path = self.main.with_file_name(name);
        self.add_at(&path, name, change)
    }

    fn add_at(
        &mut self,
        path: &Path,
        branch: &str,
        change: impl FnOnce(&mut WorktreeFacts),
    ) -> PathBuf {
        let path_text = path.to_str().expect("UTF-8");
        git(
            &self.main,
            &["worktree", "add", "--quiet", "-b", branch, path_text],
        );
        let path = path.canonicalize().expect("a real path");
        let target = path.join("target/debug");
        fs::create_dir_all(&target).expect("the directory is made");
        fs::write(target.join("app"), vec![7_u8; MEBIBYTE]).expect("a file is written");
        let mut facts = WorktreeFacts {
            merged_by: WorktreeMergedBy::Ancestor.into(),
            ignored_paths: vec!["target/".to_owned()],
            ..fact(&path, branch)
        };
        change(&mut facts);
        self.facts.push(facts);
        self.write_facts();
        path
    }

    fn change(&mut self, path: &Path, change: impl FnOnce(&mut WorktreeFacts)) {
        let path = path.display().to_string();
        let facts = self
            .facts
            .iter_mut()
            .find(|facts| facts.path == path)
            .expect("the worktree has facts");
        change(facts);
        self.write_facts();
    }

    fn write_facts(&self) {
        let response = WorktreeFactsResponse {
            base_branch: "main".to_owned(),
            last_fetch_at_ms: 0,
            worktrees: self.facts.clone(),
        };
        fs::write(
            self.sandbox.home().join(GIT_FACTS_FILE),
            response.encode_to_vec(),
        )
        .expect("the facts are written");
    }

    fn json(&self, arguments: &[&str]) -> Value {
        let mut arguments = arguments.to_vec();
        arguments.push("--json");
        let run = self.sandbox.nomux(&arguments);
        assert!(run.code <= 1, "{run:?}");
        run.json()
    }

    fn worktree_list(&self) -> String {
        git(&self.main, &["worktree", "list", "--porcelain"])
    }

    fn git_changes(&self) -> String {
        fs::read_to_string(self.sandbox.home().join(GIT_CHANGES_FILE)).unwrap_or_default()
    }
}

fn fact(path: &Path, branch: &str) -> WorktreeFacts {
    WorktreeFacts {
        path: path.display().to_string(),
        branch: branch.to_owned(),
        head: git(path, &["rev-parse", "HEAD"]).trim().to_owned(),
        last_activity_at_ms: 1,
        ..WorktreeFacts::default()
    }
}

/// The items of a plan or the results of a clean, as `(worktree name,
/// step)` with the first refusal, or `runs` / the outcome.
fn items(document: &Value) -> BTreeMap<(String, String), String> {
    let (list, field) = match document.get("plan") {
        Some(plan) => (&plan["items"], None),
        None => (&document["clean"]["results"], Some("outcome")),
    };
    list.as_array()
        .expect("a list of items")
        .iter()
        .map(|item| {
            let name = Path::new(item["worktree"].as_str().expect("a path"))
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            let value = match field {
                Some(field) => item[field].as_str().expect("an outcome").to_owned(),
                None if item["runs"] == true => "runs".to_owned(),
                None => item["refusals"][0].as_str().expect("a refusal").to_owned(),
            };
            (
                (name, item["step"].as_str().expect("a step").to_owned()),
                value,
            )
        })
        .collect()
}

fn expected(rows: &[(&str, &str, &str)]) -> BTreeMap<(String, String), String> {
    rows.iter()
        .map(|(name, step, value)| {
            (
                ((*name).to_owned(), (*step).to_owned()),
                (*value).to_owned(),
            )
        })
        .collect()
}

/// Each file and directory below `root` with its size and its modification
/// time.
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
            listing.insert(
                entry.path(),
                (metadata.len(), metadata.modified().expect("a time")),
            );
        }
    }
    listing
}

/// The worktrees of the safety table that a clean refuses, and one that it
/// cleans.
struct Cases {
    outside: PathBuf,
    done: PathBuf,
    refused: Vec<PathBuf>,
    env: PathBuf,
}

fn cases(project: &mut Project) -> Cases {
    let outside = project.sandbox.home().join("outside");
    fs::create_dir_all(&outside).expect("the directory is made");
    fs::write(outside.join("keep.txt"), "keep").expect("a file is written");
    let done = project.add("acme-done", |_| {});
    symlink(&outside, done.join("target/debug/link")).expect("a link is made");
    let dirty = project.add("acme-dirty", |facts| facts.is_dirty = true);
    let unpushed = project.add("acme-unpushed", |facts| {
        facts.merged_by = WorktreeMergedBy::UpstreamGone.into();
        facts.upstream_gone = true;
        facts.unpushed_commits = 2;
    });
    let env = project.add("acme-env", |facts| {
        facts.ignored_paths = vec![".env".to_owned(), "target/".to_owned()];
    });
    fs::write(env.join(".env"), "SECRET=1\n").expect("a file is written");
    let detached = project.add("acme-detached", |facts| {
        facts.branch = String::new();
        facts.has_detached_commits = true;
    });
    let submodule = project.add("acme-submodule", |facts| facts.has_submodules = true);
    let agent = project.main.join(".claude/worktrees/agent-1");
    let agent = project.add_at(&agent, "worktree-agent-1", |facts| {
        facts.is_locked = true;
        facts.locked_reason = "claude agent agent-1 (pid 4242)".to_owned();
    });
    git(
        &project.main,
        &[
            "worktree",
            "lock",
            "--reason",
            "claude agent agent-1 (pid 4242)",
            agent.to_str().expect("UTF-8"),
        ],
    );
    let gone = project.add("acme-gone", |facts| facts.is_missing = true);
    fs::remove_dir_all(&gone).expect("the directory is removed");
    Cases {
        outside,
        done,
        refused: vec![dirty, unpushed, env.clone(), detached, submodule, agent],
        env,
    }
}

/// The plan of [`cases`]: the first refusal of each item, or `runs`.
fn plan_of_the_cases() -> BTreeMap<(String, String), String> {
    expected(&[
        ("acme-done", "build-output", "runs"),
        ("acme-done", "trash", "runs"),
        ("acme-dirty", "build-output", "uncommitted-changes"),
        ("acme-dirty", "trash", "uncommitted-changes"),
        ("acme-unpushed", "build-output", "unpushed-commits"),
        ("acme-unpushed", "trash", "unpushed-commits"),
        ("acme-env", "build-output", "ignored-user-files"),
        ("acme-env", "trash", "ignored-user-files"),
        ("acme-detached", "build-output", "detached-commits"),
        ("acme-detached", "trash", "detached-commits"),
        ("acme-submodule", "build-output", "submodule"),
        ("acme-submodule", "trash", "submodule"),
        ("agent-1", "build-output", "locked"),
        ("agent-1", "trash", "locked"),
        ("acme-gone", "prune", "runs"),
    ])
}

#[test]
fn the_dry_run_is_the_list_that_the_clean_runs_and_the_trash_place_restores_it() {
    let mut project = Project::new(None);
    let Cases {
        outside,
        done,
        refused,
        env,
    } = cases(&mut project);
    let src = project.sandbox.home().join("src");
    let before = listing(&src);

    let plan = project.json(&["worktree", "clean", "acme", "--dry-run"]);

    assert_eq!(listing(&src), before, "the dry run changed a file");
    assert_eq!(project.git_changes(), "", "the dry run ran git");
    let planned = items(&plan);
    assert_eq!(planned, plan_of_the_cases());
    let agent_item = plan["plan"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| {
            item["worktree"]
                .as_str()
                .is_some_and(|path| path.ends_with("agent-1"))
        })
        .expect("the agent worktree");
    assert!(
        agent_item["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains("claude-code")),
        "{agent_item}"
    );

    let cleaned = project.json(&["worktree", "clean", "acme", "--yes"]);

    // The clean ran the items that the dry run listed, and only these.
    let ran: BTreeSet<_> = items(&cleaned)
        .into_iter()
        .filter(|(_, outcome)| outcome == "done")
        .map(|(key, _)| key)
        .collect();
    let runnable: BTreeSet<_> = planned
        .into_iter()
        .filter(|(_, value)| value == "runs")
        .map(|(key, _)| key)
        .collect();
    assert_eq!(ran, runnable);
    let trash_path = cleaned["clean"]["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|result| result["step"] == "trash" && result["outcome"] == "done")
        .and_then(|result| result["path"].as_str())
        .expect("the path in the trash place")
        .to_owned();
    assert!(!done.exists());
    assert!(!Path::new(&trash_path).join("target").exists());
    assert!(Path::new(&trash_path).join("Cargo.toml").exists());
    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).expect("the file outside stays"),
        "keep"
    );
    let list = project.worktree_list();
    assert!(list.contains(&format!("worktree {trash_path}\n")), "{list}");
    assert!(list.contains("locked nomux-trash"), "{list}");
    assert!(
        list.contains("locked claude agent agent-1 (pid 4242)"),
        "{list}"
    );
    assert!(!list.contains("acme-gone"), "{list}");
    for refused in &refused {
        assert!(
            refused.join("target/debug/app").exists(),
            "{}",
            refused.display()
        );
    }
    assert!(env.join(".env").exists());

    let trash = project.json(&["worktree", "trash", "ls"]);
    assert_eq!(trash["trash"][0]["path"], done.display().to_string());
    assert_eq!(trash["trash"][0]["trash_path"], trash_path.as_str());

    let restored = project.json(&["worktree", "restore", "acme-done"]);

    assert_eq!(restored["restored"]["path"], done.display().to_string());
    assert!(done.join("Cargo.toml").exists());
    assert!(!Path::new(&trash_path).exists());
    assert!(!project.worktree_list().contains("nomux-trash"));
    assert_eq!(
        project.json(&["worktree", "trash", "ls"])["trash"],
        Value::Array(Vec::new())
    );
}

#[test]
fn the_removal_of_build_output_keeps_the_ignored_files_of_the_user() {
    let mut project = Project::new(None);
    let env = project.add("acme-env", |facts| {
        facts.merged_by = WorktreeMergedBy::Unspecified.into();
        facts.ignored_paths = vec![
            ".env".to_owned(),
            "local.db".to_owned(),
            "target/".to_owned(),
        ];
    });
    fs::write(env.join(".env"), "SECRET=1\n").expect("a file is written");
    fs::write(env.join("local.db"), "rows").expect("a file is written");

    let cleaned = project.json(&["worktree", "clean", "acme", "--build-output", "--yes"]);

    assert_eq!(
        items(&cleaned),
        expected(&[("acme-env", "build-output", "done")])
    );
    assert!(!env.join("target").exists());
    assert_eq!(
        fs::read_to_string(env.join(".env")).expect(".env stays"),
        "SECRET=1\n"
    );
    assert_eq!(
        fs::read_to_string(env.join("local.db")).expect("the database stays"),
        "rows"
    );
    assert!(
        !project
            .sandbox
            .home()
            .join("src")
            .read_dir()
            .expect("src is read")
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .contains("nomux-deleting")),
        "a renamed directory stays"
    );
}

#[test]
fn a_session_a_process_and_a_build_lock_refuse_the_removal_of_build_output() {
    let mut project = Project::new(None);
    let with_session = project.add("acme-session", |_| {});
    let with_process = project.add("acme-process", |_| {});
    let with_lock = project.add("acme-lock", |_| {});
    let _ = project.sandbox.nomux_ok(&[
        "new",
        "--quiet",
        "--cwd",
        with_session.to_str().expect("UTF-8"),
        "--",
        "sleep",
        "60",
    ]);
    let mut process = Command::new("sleep")
        .arg("60")
        .current_dir(&with_process)
        .stdin(Stdio::null())
        .spawn()
        .expect("sleep starts");
    let lock_file = fs::File::create(with_lock.join("target/debug/.cargo-lock"))
        .expect("the lock file is made");
    flock(&lock_file, FlockOperation::LockExclusive).expect("the lock is taken");
    let arguments = ["worktree", "clean", "acme", "--build-output", "--dry-run"];

    let busy = items(&project.json(&arguments));
    drop(lock_file);
    process.kill().expect("sleep stops");
    process.wait().expect("sleep ends");
    let free = items(&project.json(&arguments));

    assert_eq!(
        busy,
        expected(&[
            ("acme-session", "build-output", "active-session"),
            ("acme-process", "build-output", "process-running"),
            ("acme-lock", "build-output", "process-running"),
        ])
    );
    assert_eq!(
        free,
        expected(&[
            ("acme-session", "build-output", "active-session"),
            ("acme-process", "build-output", "runs"),
            ("acme-lock", "build-output", "runs"),
        ])
    );
}

#[test]
fn the_end_of_the_grace_time_removes_the_worktree_and_deletes_only_a_branch_that_the_base_branch_has()
 {
    let mut project = Project::new(Some("[worktrees]\ngrace = \"2s\"\n"));
    let strict = project.add("acme-strict", |_| {});
    let gone = project.add("acme-gone-upstream", |facts| {
        facts.merged_by = WorktreeMergedBy::UpstreamGone.into();
        facts.upstream_gone = true;
        facts.unpushed_commits = 1;
    });
    let arguments = [
        "worktree",
        "clean",
        "acme",
        "--delete-branch",
        "--allow-unpushed",
        "--yes",
    ];

    let cleaned = project.json(&arguments);

    let trash_items: Vec<&Value> = cleaned["clean"]["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter(|result| result["step"] == "trash")
        .collect();
    let by_name = |name: &str| {
        trash_items
            .iter()
            .find(|item| {
                item["worktree"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(name))
            })
            .copied()
            .expect("an item")
    };
    assert_eq!(by_name("acme-strict")["outcome"], "done");
    assert_eq!(by_name("acme-strict")["deletes_branch"], true);
    assert_eq!(by_name("acme-gone-upstream")["outcome"], "done");
    assert_eq!(by_name("acme-gone-upstream")["deletes_branch"], false);
    assert!(
        by_name("acme-gone-upstream")["branch_kept_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("does not have each change")),
        "{cleaned}"
    );
    wait_for("the end of the grace time", || {
        (project.json(&["worktree", "trash", "ls"])["trash"] == Value::Array(Vec::new()))
            .then_some(())
    });
    let branches = git(&project.main, &["branch", "--format=%(refname:short)"]);
    assert!(!branches.contains("acme-strict"), "{branches}");
    assert!(branches.contains("acme-gone-upstream"), "{branches}");
    assert!(!strict.exists() && !gone.exists());
    let list = project.worktree_list();
    assert!(!list.contains("nomux-trash"), "{list}");
}

#[test]
fn a_deletion_that_a_stop_of_the_daemon_interrupted_ends_at_the_next_clean() {
    let mut project = Project::new(None);
    let worktree = project.add("acme-done", |_| {});
    let leftover = worktree.join("target.nomux-deleting-0123456789abcdef");
    fs::create_dir_all(leftover.join("debug")).expect("the directory is made");
    fs::write(leftover.join("debug/app"), "half").expect("a file is written");
    let state = if cfg!(target_os = "macos") {
        "Library/Application Support/nomux"
    } else {
        ".local/state/nomux"
    };
    let state = project.sandbox.home().join(state);
    fs::create_dir_all(&state).expect("the state directory is made");
    let _ = project.sandbox.nomux_ok(&["daemon", "stop"]);
    fs::write(
        state.join("worktrees.json"),
        format!(
            "{{\"version\":1,\"deleting\":[{:?}]}}",
            leftover.display().to_string()
        ),
    )
    .expect("the file is written");

    let _ = project.json(&["worktree", "clean", "acme", "--build-output", "--yes"]);

    assert!(!leftover.exists());
    assert!(!worktree.join("target").exists());
    let stored = fs::read_to_string(state.join("worktrees.json")).expect("the file is read");
    assert!(stored.contains("\"deleting\": []"), "{stored}");
}

#[tokio::test(flavor = "current_thread")]
async fn an_item_whose_worktree_a_tool_removed_after_the_plan_is_skipped_and_the_others_run() {
    let mut project = Project::new(None);
    let kept = project.add("acme-kept", |_| {});
    let swept = project.add("acme-swept", |_| {});
    let _ = project.sandbox.nomux_ok(&["worktree", "ls", "--all"]);
    let mut client = Client::connect(&project.sandbox.daemon_socket(), ClientKind::Cli)
        .await
        .expect("the client connects");
    let plan = client
        .plan_clean(&CleanRequest {
            project_id: None,
            action: CleanAction::Finished,
            worktree_paths: Vec::new(),
            delete_branch: false,
            allow_unpushed: false,
            include_ignored: false,
            now: false,
        })
        .await
        .expect("a plan");
    // The sweep of an agent tool removes a worktree after the plan.
    git(
        &project.main,
        &[
            "worktree",
            "remove",
            "--force",
            swept.to_str().expect("UTF-8"),
        ],
    );
    project.change(&swept, |facts| {
        facts.is_missing = true;
        facts.ignored_paths.clear();
    });

    let cleaned = client
        .clean_worktrees(&plan.plan_id)
        .await
        .expect("a clean");

    let outcomes: Vec<(String, PlanStep, CleanOutcome)> = cleaned
        .results
        .iter()
        .map(|result| {
            let name = Path::new(&result.item.worktree_path)
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            (name, result.item.step, result.outcome)
        })
        .collect();
    let kept_name = || "acme-kept".to_owned();
    let swept_name = || "acme-swept".to_owned();
    assert_eq!(
        outcomes,
        [
            (kept_name(), PlanStep::BuildOutput, CleanOutcome::Done),
            (kept_name(), PlanStep::Trash, CleanOutcome::Done),
            (swept_name(), PlanStep::BuildOutput, CleanOutcome::Changed),
            (swept_name(), PlanStep::Trash, CleanOutcome::Changed),
        ]
    );
    assert!(!kept.exists());
}

#[test]
fn a_moved_project_gets_a_repair_proposal_and_no_prune() {
    let mut project = Project::new(None);
    let moved = project.main.join(".claude/worktrees/agent-x");
    fs::create_dir_all(&moved).expect("the directory is made");
    fs::write(
        moved.join(".git"),
        "gitdir: /old/acme/.git/worktrees/agent-x\n",
    )
    .expect("a file is written");
    project.facts.push(WorktreeFacts {
        path: "/old/acme/.claude/worktrees/agent-x".to_owned(),
        branch: "worktree-agent-x".to_owned(),
        is_missing: true,
        ..WorktreeFacts::default()
    });
    project.facts.push(WorktreeFacts {
        path: "/old/acme/gone".to_owned(),
        branch: "gone".to_owned(),
        is_missing: true,
        ..WorktreeFacts::default()
    });
    project.write_facts();

    let plan = project.json(&["worktree", "clean", "acme", "--prune", "--dry-run"]);

    let items = plan["plan"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{plan}");
    assert_eq!(items[0]["step"], "repair");
    assert_eq!(items[0]["path"], moved.display().to_string());
    assert!(
        items[0]["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains("git worktree repair")),
        "{plan}"
    );
}

#[test]
fn the_main_worktree_loses_its_build_output_only_when_it_is_named() {
    let mut project = Project::new(None);
    let target = project.main.join("target/debug");
    fs::create_dir_all(&target).expect("the directory is made");
    fs::write(target.join("app"), vec![7_u8; MEBIBYTE]).expect("a file is written");
    let main_directory = project.main.clone();
    project.change(&main_directory, |facts| {
        facts.ignored_paths = vec!["target/".to_owned()];
    });
    let main = project.main.to_str().expect("UTF-8").to_owned();

    let all = project.json(&["worktree", "clean", "acme", "--build-output", "--dry-run"]);
    let named = project.json(&[
        "worktree",
        "clean",
        "acme",
        "--build-output",
        "--worktree",
        &main,
        "--dry-run",
    ]);
    let finished = project.json(&[
        "worktree",
        "clean",
        "acme",
        "--worktree",
        &main,
        "--dry-run",
    ]);

    assert_eq!(items(&all), BTreeMap::new());
    assert_eq!(items(&named), expected(&[("acme", "build-output", "runs")]));
    assert_eq!(
        items(&finished),
        expected(&[
            ("acme", "build-output", "main-worktree"),
            ("acme", "trash", "main-worktree"),
        ])
    );
}

#[test]
fn a_dormant_session_warns_a_command_and_stops_a_rule_of_the_settings() {
    let mut project = Project::new(None);
    let dormant = project.add("acme-dormant", |_| {});
    let free = project.add("acme-free", |_| {});
    let _ = project.sandbox.nomux_ok(&[
        "new",
        "--quiet",
        "--cwd",
        dormant.to_str().expect("UTF-8"),
        "--",
        "sleep",
        "600",
    ]);
    project.sandbox.kill_each_process();
    project
        .sandbox
        .write_configuration("[worktrees]\nauto = [\"build-output-of-finished\"]\n");

    // The daemon starts again: the rule runs at its start and after a list.
    let _ = project.sandbox.nomux_ok(&["worktree", "ls", "acme"]);
    wait_for("the rule of the settings", || {
        (!free.join("target").exists()).then_some(())
    });
    let plan = project.json(&[
        "worktree",
        "clean",
        "acme",
        "--build-output",
        "--worktree",
        dormant.to_str().expect("UTF-8"),
        "--dry-run",
    ]);

    assert!(dormant.join("target/debug/app").exists());
    let item = &plan["plan"]["items"][0];
    assert_eq!(item["runs"], true, "{plan}");
    assert!(
        item["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains("dormant")),
        "{plan}"
    );
}

#[test]
fn a_mark_keep_protects_a_worktree_and_a_mark_done_finishes_one() {
    let mut project = Project::new(None);
    let kept = project.add("acme-kept", |_| {});
    let open = project.add("acme-open", |facts| {
        facts.merged_by = WorktreeMergedBy::Unspecified.into();
    });
    let _ = project
        .sandbox
        .nomux_ok(&["worktree", "mark", "acme-kept", "keep"]);
    let _ = project
        .sandbox
        .nomux_ok(&["worktree", "mark", "acme-open", "done"]);

    let plan = project.json(&["worktree", "clean", "acme", "--dry-run"]);
    let _ = project
        .sandbox
        .nomux_ok(&["worktree", "unmark", "acme-kept"]);
    let unmarked = project.json(&["worktree", "clean", "acme", "--dry-run"]);

    assert_eq!(
        items(&plan),
        expected(&[
            ("acme-kept", "build-output", "kept"),
            ("acme-kept", "trash", "kept"),
            ("acme-open", "build-output", "runs"),
            ("acme-open", "trash", "runs"),
        ])
    );
    assert_eq!(
        items(&unmarked)[&("acme-kept".to_owned(), "trash".to_owned())],
        "runs"
    );
    assert!(kept.exists() && open.exists());
}
