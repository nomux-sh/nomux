# Worktrees

This document describes the worktrees of the Projects in the core: the model, the states and the flags and how the daemon finds each one, the sizes and how the daemon measures them, the checks of `host doctor`, and what this version does not do.
`spec/worktree.md` of the [`proto`](https://github.com/nomux-sh/proto) repository specifies the Worktree service and the method `worktree_facts` of the git extension: the fields, the rules of the states, of the flags, of the merge rules and of the refusals. This document does not repeat the tables of the specification.

This version only reads. Nothing removes, moves or changes a worktree, a branch or a file of a repository. `nomux worktree ls` answers the question "what is on my disk, and what can go".

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
- The daemon keeps no list of worktrees. It asks the extension at each `Worktree.List`, one call for each Project. It keeps only the sizes, in memory.
- nomux never fetches. `last fetch` in the total line is the time of `FETCH_HEAD`.

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
| Busy | daemon | a process has its current directory in the worktree: `/proc/<pid>/cwd` on Linux. The daemon and its children (the holders, the extensions) do not count: they have the current directory of the daemon. On macOS the daemon does not see the current directories, thus a worktree is never busy there |
| Other file system | daemon | the directory has a different device than its parent directory |
| Ignored user files | daemon | an ignored path that the build-output rules do not accept, or more ignored paths than the extension gives (256) |
| Size | daemon | the size walk: see [Sizes](#sizes) |
| Free space | daemon | `statvfs` of the Project directory: the bytes for a user without special rights |

The extension and the daemon read only. The extension writes no object (the merge check keeps its objects in memory), no index and no reference, and it starts no `git` process. The daemon reads directories and the metadata of files, and reads only `CACHEDIR.TAG` files: the first 64 bytes.

## States, flags and refusals

The rules are in `spec/worktree.md`. The daemon applies them in `crates/nomuxd/src/worktrees/state.rs`:

- **State:** `active` with a session, else `finished` if a merge rule matches, else `stale` after 14 days without activity, else `idle`.
- **Flags:** each fact of the table above that is true.
- **Refusals:** why a removal of the directory would lose something, the most important first. The CLI shows the first one in the `REASON` column. A worktree without a refusal is `yes`: it is finished, and a removal of its directory loses no work.

The refusal `unpushed` does not apply when the base branch has each change of the branch (`ANCESTOR` or `TREE`): the commits are not pushed, but their changes are in the base branch.

A branch without a commit of its own is in the base branch, thus it is `finished`. A new worktree in which nothing was done is `finished` and loses nothing.

## Build output

A directory is build output only if git ignores all of it (the extension gives it as one ignored path that ends with `/`) and a rule accepts it. The rules are data: `RULES` in `crates/nomuxd/src/worktrees/build_output.rs`.

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
- A size is old after 1 hour, or when a session that was in the worktree at the walk ended: a build probably changed the build output.
- **The daemon has no timer for the sizes.** A daemon that no client asks measures nothing, thus its CPU time is 0 when it is idle. A size is measured again only when a client asks for the list and the size is old.
- One walk runs at a time, on a thread of the blocking pool of tokio (`spawn_blocking`) that ends some seconds after the walk. After each 2000 entries, the walk pauses for three times the time that it worked: it uses a quarter of one processor at most. The walk does not lower its priority with `setpriority`: the thread of the pool runs other blocking work later and could not get its priority back. `ioprio_set` needs `unsafe` code.
- The sizes are in memory only. A restart of the daemon measures again.

## Commands

[cli.md](cli.md) has the options, the text output and the JSON document of `worktree ls`.

```console
$ nomux worktree ls --wait
WORKTREE         BRANCH            STATE     SIZE    BUILD  AGE  REASON
acme
  acme           main              idle      480.0K  0B     3m   no: main worktree
  acme-auth      auth-flow         finished  2.8M    2.8M   3m   yes: merged
  acme-detached  -                 idle      28.0K   0B     3m   no: unpushed 1
  acme-fix       fix-cache         finished  1.9M    1.9M   3m   no: uncommitted changes
  acme-gone      gone              idle      ?       ?      3m   no: directory missing
  acme-old       old-ui            finished  28.0K   0B     3m   no: ignored files (.env)
  acme-spike     spike             idle      28.0K   0B     3m   no: uncommitted changes
  agent-1        worktree-agent-1  finished  24.0K   0B     3m   no: locked (claude agent agent-1 (pid 4242))
8 worktrees, 5.3M (build output 4.7M), 1 can be removed (2.8M), 28.7G free, last fetch 3m ago
```

`host doctor` has two checks of the worktrees, for the local host and for each remote host whose daemon answers:

| Check | Result |
|---|---|
| `free-space` | The smallest free space of the file systems of the Projects. Below 10 GiB a warning, below 3 GiB the check fails |
| `shared-build-output` | Advice when two or more worktrees have their own build output: for cargo, a shared build directory (`CARGO_BUILD_BUILD_DIR` or `build.build-dir`; measured: a second worktree of a project with 79 units built in 0.55 s instead of 17.4 s and added 10 MiB instead of 354 MiB, but builds wait for one lock); for `node_modules`, pnpm or bun, which link the files of a package from one store. The check changes no file |

## Code

| Part | Responsibility |
|---|---|
| `crates/nomuxd/src/worktrees.rs` | `Worktree.List`: the Projects, the facts, the sessions, the processes, the rules, the sizes |
| `crates/nomuxd/src/worktrees/facts.rs` | The call of `worktree_facts` and the reason for the user when it fails |
| `crates/nomuxd/src/worktrees/state.rs` | The state, the flags and the refusals |
| `crates/nomuxd/src/worktrees/build_output.rs` | The build-output rules |
| `crates/nomuxd/src/worktrees/host_facts.rs` | The current directories of the processes, the devices, the free space |
| `crates/nomuxd/src/worktrees/sizes.rs`, `size_walk.rs` | The sizes in memory and the walk |
| `crates/nomux-client/src/worktree.rs` | `Client::worktrees` and the types of the list |
| `crates/nomux-client/src/doctor/worktrees.rs` | The checks `free-space` and `shared-build-output` |
| `crates/nomux-cli/src/worktree_commands.rs`, `output/worktree_human.rs`, `output/worktree_json.rs` | `worktree ls` |

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

## Not done in this version

- **Removal.** `worktree clean` (build output, finished worktrees, prune), the trash with its grace time, `restore`, `mark`, the policy settings and the automatic rules are step 2. The refusals of this version are the checks of that removal.
- **Events.** No `WorktreeChanged` and no event for the free space; the sidebar line of a worktree. Step 2.
- **Settings.** The stale time (14 days), the size lifetime (1 hour), the free-space levels (10 GiB, 3 GiB) and the base branch (`origin/HEAD`, `main`, `master`) are constants. The `[worktrees]` table of the configuration file and more build-output rules from the user come with the removal.
- **Fetch.** `worktree ls --fetch` and a forge answer for "merged". A squash merge whose lines the base branch changed later is found only after a fetch with prune of the user (`upstream gone`).
- **The tool of a worktree.** The `TOOL` column (Claude Code, Codex, Cursor) and the protection of a worktree from the sweep of a tool.
- **Busy builds.** A held lock of a build directory is not checked; only the current directory of a process is. macOS has no busy check.
- **Shared build output.** The `[worktrees.env]` setting for new sessions and `worktree new --seed-from` are step 3.
