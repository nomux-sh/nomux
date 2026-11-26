# Worktrees

This document describes the worktrees of the Projects in the core: the model, the states and the flags and how the daemon finds each one, the sizes and how the daemon measures them, the clean, the trash place and how to undo a clean, the settings, the events, the checks of `host doctor`, and what this version does not do.
`spec/worktree.md` of the [`proto`](https://github.com/nomux-sh/proto) repository specifies the Worktree service and the methods of the git extension: the fields, the rules of the states, of the flags, of the merge rules, of the refusals, of the plan and of the clean. This document does not repeat the tables of the specification.

`nomux worktree ls` answers the question "what is on my disk, and what can go". `nomux worktree clean` removes it. **By default nothing is removed without a command of the user**: the daemon only reports, until the user turns on a rule in the settings.

## The git extension is necessary

The daemon has no git code. The git facts of a worktree (its branch, its changes, its commits that are not pushed, the merge rules) come from the method `worktree_facts` of the git extension `nomux.git`, which reads the repository with `gix`. Thus the daemon does not get a git library with its memory and its start time, and the rules that read git have one place: the extension, which the desktop client also uses for its git panel.

Without the extension, or when its approved directories do not contain the directory of a Project, `nomux worktree ls` shows the Project without worktrees and the reason, with the command that installs or approves the extension:

```console
$ nomux worktree ls
WORKTREE  BRANCH  STATE  SIZE  BUILD  AGE  REASON
acme
0 worktrees, 0B (build output 0B), 0 can be removed (0B), 30.9G free, last fetch never
acme: the git extension nomux.git is not installed. Install it with `nomux ext install git --grant-all --grant fs=/home/user/src/acme`
```

A linked worktree outside the approved directories is in the list with the facts of the common git directory (its branch, its `HEAD`, its lock) and with the refusal `facts unknown`. Approve a directory that contains the Project and its worktrees.

## Model

- A **worktree** of a Project is a directory that git lists for the repository of the Project directory: the main worktree and each linked worktree, wherever the directory is. A worktree that an agent tool made (for example `.claude/worktrees/<name>` of Claude Code) is an ordinary linked worktree and gets the same rules.
- A **session** belongs to the worktree with the longest directory that contains its resolved working directory. A worktree below the main worktree has its own sessions.
- The daemon keeps no list of worktrees. It asks the extension at each `Worktree.List`, one call for each Project. It keeps the sizes in memory, and the marks, the trash entries and the build output directories whose deletion runs in `worktrees.json` in its state directory.
- The daemon never fetches. `last fetch` in the total line is the time of `FETCH_HEAD`. Only `worktree ls --fetch` and `worktree clean --fetch` run `git fetch --prune` of the user, in the CLI, on the local host: the only use of the network.
- The `TOOL` column (`--json`: `tool`) is the agent tool that probably made the worktree, from a table of directories in `crates/nomuxd/src/worktrees/tools.rs`: `.claude/worktrees/` is `claude-code`, `.codex/worktrees/` or `$CODEX_HOME/worktrees` is `codex`, `.cursor/worktrees/` is `cursor`, `orca/workspaces/` is `orca`. It is a guess. A plan warns that the tool may keep a record of such a worktree.

## Where each fact comes from

| Fact | Source | How |
|---|---|---|
| The worktrees, branch, `HEAD`, main, bare | extension | `HEAD` and `gitdir` of each worktree in the common git directory |
| Locked and the reason | extension | the `locked` file of the linked worktree |
| Missing directory | extension | the directory of `gitdir` does not exist |
| Submodule | extension | a `.gitmodules` file in the worktree |
| Dirty | extension | one status walk of `gix`: a staged, unstaged or untracked change that is not ignored, a conflict, or an operation in progress |
| Ignored paths | extension | the same status walk: each ignored path, an ignored directory as one path |
| Commits not pushed | extension | the commits of `HEAD` that no remote-tracking branch has; without one, the commits that the base branch does not have |
| Detached commits | extension | a detached `HEAD` with a commit that no branch has |
| Upstream gone | extension | the branch has an upstream branch in its configuration, and the remote-tracking branch does not exist |
| Merged | extension | the first of `ANCESTOR`, `TREE` (a merge in memory gives the tree of the base branch) and `UPSTREAM_GONE` |
| Last activity | extension | the newest modification time of `HEAD`, `index` and `logs/HEAD` in the git directory of the worktree |
| Active | daemon | a session that is not ended (also a dormant one) in the worktree |
| Busy | daemon | a process has its current directory in the worktree: `/proc/<pid>/cwd` on Linux, `proc_pidinfo` with `PROC_PIDVNODEPATHINFO` on macOS. The daemon and its children (the holders, the extensions) do not count: they have the current directory of the daemon. Also busy: a build holds a lock in a build output directory, `<dir>/<profile>/.cargo-lock` or `<dir>/<target>/<profile>/.cargo-lock`. The daemon tests each lock file with a shared `flock` that does not wait and gives it back at once |
| Mark | daemon | `worktree mark <w> done` or `keep`, in `worktrees.json` |
| In the trash | daemon | the worktree is locked with the reason `nomux-trash` |
| No trash place | daemon | the parent directory of the worktree is not writable |
| Other file system | daemon | the directory has a different device than its parent directory |
| Ignored user files | daemon | an ignored path that the build-output rules do not accept, or more ignored paths than the extension gives (256) |
| Size | daemon | the size walk: see [Sizes](#sizes) |
| Free space | daemon | `statvfs` of the Project directory: the bytes for a user without special rights |

For a list, the extension and the daemon read only. The extension writes no object (the merge check keeps its objects in memory), no index and no reference, and it starts no `git` process. The daemon reads directories and the metadata of files, and reads only `CACHEDIR.TAG` files: the first 64 bytes.

## States, flags and refusals

The rules are in `spec/worktree.md`. The daemon applies them in `crates/nomuxd/src/worktrees/state.rs`:

- **State:** `active` with a session, else `finished` if a merge rule matches or the mark is `done`, else `stale` after the stale time without activity (`stale_after`, 14 days), else `idle`.
- **Flags:** each fact of the table above that is true.
- **Refusals:** why a removal of the directory would lose something, the most important first. The CLI shows the first one in the `REASON` column. A worktree without a refusal is `yes`: it is finished, and a removal of its directory loses no work.

The refusal `unpushed` does not apply when the base branch has each change of the branch (`ANCESTOR` or `TREE`): the commits are not pushed, but their changes are in the base branch.

A branch without a commit of its own is in the base branch, thus it is `finished`. A new worktree in which nothing was done is `finished` and loses nothing.

## Build output

A directory is build output only if git ignores all of it (the extension gives it as one ignored path that ends with `/`, thus no tracked file is below it) and a rule accepts it. The rules are data: `RULES` in `crates/nomuxd/src/worktrees/build_output.rs`, and the rules of the user in `[worktrees.build_output]`.

| Rule | Names | Accepted if |
|---|---|---|
| `cargo-target` | `target` | `Cargo.toml` beside it, or a valid `CACHEDIR.TAG` in it |
| `node-modules` | `node_modules` | `package.json` beside it |
| `javascript-build-cache` | `.next`, `.nuxt`, `.svelte-kit`, `.turbo`, `.parcel-cache` | `package.json` beside it |
| `python-virtual-environment` | `.venv`, `venv` | `pyvenv.cfg` in it |
| `python-cache` | `__pycache__`, `.mypy_cache`, `.ruff_cache`, `.pytest_cache` | the name |
| `cache-directory-tag` | each name | a valid `CACHEDIR.TAG` in it (the first line is `Signature: 8a477f597d28d172789f06886806bc55`) |

`dist`, `build` and `out` are not build output: some repositories keep source files there. Each other ignored path, for example `.env` or `local.db`, is a file of the user and gives the refusal `ignored files`. `git clean -X` would delete these files, and `git worktree remove` deletes them without a message: this is why the rule list decides and not git.

## Sizes

- The size of a worktree is the allocated size (`st_blocks` × 512) of each entry below its directory. The walk follows no symbolic link and does not go into a different file system.
- The daemon measures all worktrees of a Project in one walk. A file with more than one hard link counts one time in that walk: the first worktree that the walk measures gets its bytes. Package managers link files (bun, pnpm, uv), thus the sum of the sizes of single worktrees can be much larger than the real cost.
- A nested worktree (for example a worktree of an agent tool below the main worktree) is not in the size of the worktree around it.
- Each build output directory is also measured alone: `BUILD` in the table, `build_outputs` in the JSON document.
- A file whose blocks are shared with a different file (a reflink, for example a copy with `cp --reflink`) counts in full: the walk cannot see shared blocks. Thus the sizes are an upper bound; the JSON document has `shared_extents: "unknown"`.

When the daemon measures:

- Never on the path of a request. `Worktree.List` answers with the sizes that it has. The table marks a size that is not measured yet with `?`, and an old size with `*`.
- After the answer, the daemon measures each Project with a size that is absent or old. With `worktree ls --wait`, it measures before the answer.
- A size is old after the size lifetime (`size_ttl`, 1 hour), or when a session that was in the worktree at the walk ended: a build probably changed the build output. A clean forgets the size of each worktree that it changed.
- **The daemon has no timer for the sizes.** A daemon that no client asks measures nothing, thus its CPU time is 0 when it is idle. A size is measured again only when a client asks for the list and the size is old.
- One walk runs at a time, on a thread of the blocking pool of tokio (`spawn_blocking`) that ends some seconds after the walk. After each 2000 entries, the walk pauses for three times the time that it worked: it uses a quarter of one processor at most. The walk does not lower its priority with `setpriority`: the thread of the pool runs other blocking work later and could not get its priority back. `ioprio_set` needs `unsafe` code.
- The sizes are in memory only. A restart of the daemon measures again.

## The clean

`nomux worktree clean` makes a plan with the daemon, shows it, and then runs exactly the items of that plan. A plan and a clean use the same code (`crates/nomuxd/src/worktrees/plan.rs`): the dry run is the list of what the clean does.

- Without `--yes`, the command shows the plan: each path, its size and the reason. On a terminal it then asks one question with the number of items and the bytes. Without a terminal, or with `--dry-run`, it changes nothing.
- With `--yes`, or after the answer `y`, the CLI sends the identifier of the plan. The daemon keeps a plan for 15 minutes.
- Before each item the daemon makes the plan again with the same request. The item runs only when the new plan has the same item (the worktree, the step, the path and `HEAD`) without a refusal. Else the item is `skipped` (the facts changed) or `refused`, and the other items continue. An item that only the new plan has does not run.
- One clean runs at a time. A second one gets an error.
- Exit status 0 when each item ran, 1 when an item was refused, skipped or failed.

### The sequence of a clean of a worktree

1. **Build output.** Each build output directory of the worktree is renamed to `<name>.nomux-deleting-<id>` in the same parent directory, then deleted. The daemon writes the new name to `worktrees.json` before the rename: a deletion that a stop of the daemon interrupted continues at the start of the next clean. The deletion follows no symbolic link and does not go into a different file system. An ignored path that no rule accepts (`.env`, a local database, an editor directory) is never deleted here.
2. **The trash place** (`--finished`). The daemon makes `<parent>/.nomux-trash` (mode 0700) in the parent directory of the worktree, and the git extension runs `git worktree move <worktree> <parent>/.nomux-trash/<name>-<id>` and `git worktree lock --reason nomux-trash`. The move is a rename on the same file system: it takes milliseconds, also for gigabytes. The branch stays. A worktree whose directory is a mount point or on a different file system than its parent has no trash place: it is refused (`other file system`) unless `--now`, the same for a parent directory that is not writable (`no trash place`).
3. **The branch** (`--delete-branch`). Only at the end of the grace time, after the removal of the worktree, and only when the base branch still has each change of the branch (`ANCESTOR` or `TREE`) and the tip is still the `HEAD` of the plan. A branch that only has a gone upstream (a squash merge with later changes of the same lines) stays. The extension runs `git branch -D` after this check, because `git branch -d` refuses a branch that a rebase merge put on the base branch.
4. **Records of missing directories.** `git worktree prune`, when the records to prune are the ones of the plan. When a directory with the `.git` file of a missing record exists below the main worktree at the same relative path (the Project moved), the plan proposes `git worktree repair <path>` and prunes nothing in that repository.

The actions: `--build-output` (step 1 for each linked worktree; the main worktree only with `--worktree`), `--stale` (step 1 for each stale worktree), `--finished` (the default: steps 1, 2 and 4 for each finished worktree), `--prune` (step 4 only).

### What a clean never does

A clean never removes these worktrees, and refuses them with the reason. A command of the user and a rule of the settings get the same refusals. In the `--finished` action the build output of a refused worktree also stays: a refused worktree does not change.

| Case | Refusal | The one option that removes it, for a command only |
|---|---|---|
| Changes that are not committed, a conflict, an operation in progress | `uncommitted changes` | none |
| Commits that are not pushed, when the base branch does not have their changes | `unpushed` | `--allow-unpushed`: the branch keeps the commits |
| A detached `HEAD` with commits that no branch has | `detached commits` | none: make a branch first |
| A lock that nomux did not set (also the lock of an agent tool) | `locked` | none: nomux never removes or releases a lock of a different owner |
| A submodule | `submodule` | none |
| A session with a process, or a process or a build inside | `session running`, `process running` | none |
| Ignored files that are not build output | `ignored files (.env, …)` | `--include-ignored`: they go to the trash place and are deleted at the end of the grace time |
| The main worktree | `main worktree` | none (its build output only with `--worktree`) |
| A missing directory | `directory missing` | none: only its record is pruned |
| A mark `keep` | `marked keep` | none: `worktree unmark` first |

A dormant session (no process) does not refuse an item of a command: the plan has a warning, because a restore of the session will find no directory. For a rule of the settings it refuses. The removal of build output alone is not refused for changes that are not committed: git ignores all of it.

The git extension checks the same facts again before each change of git, and git refuses a dirty worktree itself: no command passes `--force`.

### How to undo

| Step | Undo |
|---|---|
| Build output | Build again. There is no grace time: to free the space is the aim |
| The trash place | `nomux worktree restore <name>` during the grace time: `git worktree unlock` and `git worktree move` back. The name is the identifier of the entry, the path of the worktree, or its last part |
| The branch | `git branch <name> <hash>`: the hash is in `worktree trash ls --json` and in the plan |
| A pruned record | Nothing to undo: the directory was gone |

`nomux worktree trash ls` lists the entries with the time that is left. `nomux worktree trash empty [<id>]...` removes entries now, with the checks of the end of the grace time. At the end of the grace time the daemon removes the worktree with the extension: `git worktree unlock`, then `git worktree remove` without `--force`. A removal that the extension refuses (for example a new `.env` in the trash place) keeps the entry, and the daemon does not try it again until it starts again.

## Settings

The global configuration file has the `[worktrees]` table. Each value is the safe default:

```toml
[worktrees]
auto = []                  # any of: "build-output-of-finished", "finished", "stale-build-output"
stale_after = "14d"
grace = "24h"
size_ttl = "1h"
base = ""                  # empty: origin/HEAD, main, master
[worktrees.disk]
warn_free = "10G"
critical_free = "3G"
[worktrees.build_output]
extra = []                 # for example { id = "dist", names = ["dist"], beside = ["package.json"] }
```

- `auto` are the cleans that the daemon runs without a command: at its start, after each `Worktree.List`, and when the free space goes below a level. Empty by default: the daemon only reports. A rule runs the same plan with the refusals of a rule of the settings, and each of its changes is an event with `automatic`.
- `base` replaces the base branch of the merge rules when the branch exists: first as a remote-tracking branch, then as a local branch.
- `extra` adds build-output rules: a directory with one of `names` is build output when a file of `beside` is in its parent directory or a file of `inside` is in it, and git ignores all of it.
- A value that is not valid is an error that names the key; the daemon then uses the defaults. The daemon reads the file at its start.
- The file has no per-Project override: nomux has no Project file yet.

## Events

The daemon writes `WorktreeChanged` to the event log for each change of a clean, a restore, the end of a grace time and a mark, and `WorktreeDiskPressure` when the free space of the file system of a Project goes below `warn_free` or `critical_free`, or above it again. The event has the free bytes, the bytes that a clean of finished worktrees could remove, and the command `nomux worktree clean --all --dry-run`. It deletes nothing.

The daemon reads the free space at each `Worktree.List`, after a clean, and every 5 minutes only while a client has a subscription to the events. A daemon without a subscription, without a trash entry and without a rule has no timer. The CLI and the clients show these events in a later version.

## Commands

[cli.md](cli.md) has the options, the text output and the JSON documents of `worktree ls`, `clean`, `restore`, `trash`, `mark` and `unmark`.

```console
$ nomux worktree clean acme --dry-run
WORKTREE    STEP          PATH                              SIZE   REASON
acme-dirty  build output  target                            2.8M   no: uncommitted changes
acme-dirty  to trash      .nomux-trash/acme-dirty-d14554b0  20.0K  no: uncommitted changes
acme-done   build output  target                            2.8M   yes: cargo-target
acme-done   to trash      .nomux-trash/acme-done-edf510c7   20.0K  yes: branch done stays
acme-env    build output  target                            2.8M   no: ignored files
acme-env    to trash      .nomux-trash/acme-env-1e0eb466    24.0K  no: ignored files (.env)
acme-gone   prune record  -                                 -      yes: the directory is missing
3 of 7 items run, 2.8M comes back (upper bound), the trash place keeps the worktrees for 1d.
Nothing changed. Run the command again with --yes to clean these items.
```

`host doctor` has two checks of the worktrees, for the local host and for each remote host whose daemon answers:

| Check | Result |
|---|---|
| `free-space` | The smallest free space of the file systems of the Projects. Below 10 GiB a warning, below 3 GiB the check fails. The check uses these defaults, not the settings of a daemon |
| `shared-build-output` | Advice when two or more worktrees have their own build output: for cargo, a shared build directory (`CARGO_BUILD_BUILD_DIR` or `build.build-dir`; measured: a second worktree of a project with 79 units built in 0.55 s instead of 17.4 s and added 10 MiB instead of 354 MiB, but builds wait for one lock); for `node_modules`, pnpm or bun, which link the files of a package from one store. The check changes no file |

## Code

| Part | Responsibility |
|---|---|
| `crates/nomuxd/src/worktrees.rs` | The snapshot of the Projects (facts, sessions, processes, rules, sizes), `Worktree.List` and `Worktree.Mark` |
| `crates/nomuxd/src/worktrees/facts.rs`, `git_changes.rs` | The calls of the git extension: the facts, and the changes of git of a clean |
| `crates/nomuxd/src/worktrees/state.rs` | The state, the flags and the refusals |
| `crates/nomuxd/src/worktrees/build_output.rs` | The build-output rules and the test of a build lock |
| `crates/nomuxd/src/worktrees/host_facts.rs`, `tools.rs` | The current directories of the processes, the devices, the free space, the tool of a worktree |
| `crates/nomuxd/src/worktrees/sizes.rs`, `size_walk.rs` | The sizes in memory and the walk |
| `crates/nomuxd/src/worktrees/plan.rs`, `clean.rs`, `deletion.rs` | `Worktree.Plan`, `Worktree.Clean`, the deletion of build output |
| `crates/nomuxd/src/worktrees/store.rs`, `trash.rs` | `worktrees.json`, `Worktree.Trash`, `Worktree.Restore`, `Worktree.EmptyTrash` |
| `crates/nomuxd/src/worktrees/disk.rs`, `background.rs` | The levels of the free space, the end of the grace time, the rules of the settings |
| `crates/nomux-core/src/configuration/worktree_settings.rs` | The `[worktrees]` table |
| `crates/nomux-client/src/worktree.rs`, `worktree_clean.rs` | The types and the requests |
| `crates/nomux-client/src/doctor/worktrees.rs` | The checks `free-space` and `shared-build-output` |
| `crates/nomux-cli/src/worktree_commands.rs`, `worktree_commands/clean.rs`, `output/worktree_*.rs` | The `worktree` commands |

The changes of git are in the git extension: `worktree_move_to_trash`, `worktree_move_from_trash`, `worktree_delete` and `worktree_prune` of `nomux.git`.

### `worktrees.json`

```json
{"version": 1,
 "marks": {"/home/user/src/acme-spike": "keep"},
 "trash": [{"id": "acme-auth-1a2b3c4d", "project_id": "w7mdj3j7icns", "repository": "/home/user/src/acme",
            "path": "/home/user/src/acme-auth", "trash_path": "/home/user/src/.nomux-trash/acme-auth-1a2b3c4d",
            "branch": "auth-flow", "head": "9df43faf…", "trashed_at_ms": 1795707021434,
            "expires_at_ms": 1795793421434, "deletes_branch": false, "allowed_ignored_paths": ["target/"],
            "allow_unpushed": false}],
 "deleting": []}
```

The daemon writes the file in one step: a new file, then a rename. A file that the daemon cannot read gives an empty store.

## Measurements

Measured on Linux (Fedora 44, XFS, 8 cores, a load average of about 12) with a release build: 5 Projects with 10 worktrees each (the main worktree and 9 linked worktrees), each linked worktree with a `target` of 200 files, 12 980 entries in all. The git extension is `nomux.git` with `worktree_facts`.

| Measurement | Value |
|---|---|
| Resident memory of the daemon with the 5 Projects, before a list | 5.0 MB |
| The same after `worktree ls --all --wait`, with the 50 sizes in memory | 5.3 MB |
| CPU time of the daemon in 30 s idle, after the lists | 0 clock ticks |
| Threads of the daemon 75 s after the last list | 1 (the walk thread and the extension process are gone) |
| `worktree ls --all` with a new daemon and `--wait`: extension start, facts, walk of the 50 worktrees | 330 ms |
| `worktree ls --all` with the sizes in memory, median of 10 | 59 ms |
| Size of the `nomuxd` binary | 2.6 MB |

The targets of [bench/README.md](../bench/README.md) hold: an idle daemon under 10 MB and with 0 CPU time. The list does not touch the start of a session.

Measurements of the clean, on the same computer (a load average of about 2), with the release binaries and the git extension `nomux.git` with the methods of a clean:

| Measurement | Value |
|---|---|
| `worktree clean --worktree <w> --yes` of one finished worktree without build output: the plan with the facts and the sizes, the plan again before the item, the move and the lock | 137 to 200 ms, median 140 ms of 5 |
| `git worktree move` and `git worktree lock` alone, for the same worktree | 3 ms |
| `worktree restore <name>` | 6 to 9 ms, median 7 ms of 5 |
| `worktree clean --yes` of 20 finished worktrees, each with a `target` of 400 files of 128 KiB (50 MiB): 41 items | 2.45 s |
| Free space after that clean, by `df` | 1000 MiB more; the daemon reported 1000.1 MiB, the plan 1001.5 MiB |
| CPU time of the daemon in 30 s idle, settings with `auto = []`, empty trash place | 0 clock ticks, 1 thread, 6.2 MB resident |
| The same with one trash entry: a timer that sleeps until the end of the grace time | 0 clock ticks |

The removal of build output deletes each file, thus its time grows with the number of files; the move to the trash place is a rename and does not.

## Not done in this version

- **Shared build output.** The `[worktrees.env]` setting for new sessions and `worktree new --seed-from`, `worktree rm`, a disk budget for each Project, and a forge extension for "merged" are step 3.
- **The events in the clients.** The daemon writes `WorktreeChanged` and `WorktreeDiskPressure`; `nomux events`, the client library and the sidebar show them in step 3.
- **A per-Project override of the settings.** nomux has no Project file yet.
- **A rule at the end of a session.** The rules of the settings run at the start of the daemon, after a list and when the free space goes below a level, not when a session ends.
