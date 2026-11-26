# AGENTS.md

Rules for coding agents and for people who work in this repository. `CLAUDE.md` and `QWEN.md` refer to this file.

## This repository

`nomux-sh/nomux` is the nomux core: the session holder (`nomux-holder`), the daemon (`nomuxd`), the CLI (`nomux`) and the libraries that they share. It is one Cargo workspace. [docs/architecture.md](docs/architecture.md) shows the components.

| Path | Responsibility |
|---|---|
| `crates/nomux-core` | Types that more than one crate uses. Now: the `--version` output of the three binaries, the lookup of a binary of the same installation, the parts of the global configuration file (`config.toml`) that the core reads: the hosts, the default restore policy and the retention time of an ended session, the data directory of nomux, and the rules of a hook event that the CLI and the daemon share. |
| `crates/nomux-term` | Abstraction over the VT library (`alacritty_terminal`): screen state, redraw sequence, OSC sequences. |
| `crates/nomux-holder-protocol` | Contract between a holder and the daemon: control messages, ring buffer layout, discovery files. |
| `crates/nomux-holder` | Binary `nomux-holder`: owns the PTY of one session. |
| `crates/nomux-transport` | Unix socket, SSH channels, QUIC streams, credit windows. Now: the socket of the daemon on the local host, the SSH client (`~/.ssh/config`, known hosts, authentication, channels to the socket of a remote daemon), the framing of the control channel and the framing of the terminal channels of an attach. |
| `crates/nomux-status` | Agent status engine: the status model, the resolver that merges the sources in their fixed sequence, the Child work tree, the aggregation rules, the rule for a session without a process and the adapter source with its rules for a soft working state. Pure code: no I/O, no async runtime. [docs/status.md](docs/status.md) describes it. |
| `crates/nomux-ext-host` | Extension host of the daemon: the installed extensions, the process of each extension in use, calls, events (also to the embedder) and the isolation of failures. [docs/extensions.md](docs/extensions.md) describes it. |
| `crates/nomuxd` | Binary `nomuxd`: the daemon: the holders, the attach of a client, the journal and the state directory of each session, the dormant and the ended sessions, the restore and the memory of the processes of each session. It embeds the extension host, serves the `Ext` service and supplies the host services of the extensions. It keeps the status of each session from its sources, serves the `Agent` and `Events` services and keeps the event log, with the events of the extensions. It serves the `Hooks` service: it gives the hook events of an agent tool to its adapter extension. [docs/daemon.md](docs/daemon.md) and [docs/adapters.md](docs/adapters.md) describe it. |
| `crates/nomux-client` | Protocol client for the CLI, the desktop client and the mobile clients: the local host and remote hosts, the installation of the binaries on a remote host, the requests for sessions, for extensions, for the status of the sessions, for the hooks and for the daemon, the events of a host, the install of the hooks of a tool in its settings file, the checks of a host, the scrollback of a session, the memory of each session, and the terminal of an attach as a stream of output and a sink for input, with its reconnect. It has no terminal handling. [docs/remote.md](docs/remote.md) describes the remote hosts. |
| `crates/nomux-cli` | Binary `nomux`: the CLI, and the service files that start the daemon at login. [docs/cli.md](docs/cli.md) has the commands, also the `ext` commands for extensions, the status commands, the `hook` and `hooks` commands, the `--host` option, the keys of an attach, the JSON schema and the exit codes. |
| `xtask` | Developer tasks. `cargo xtask ci` runs the checks. `cargo xtask fuzz` runs one fuzz target. |
| `bench` | Performance targets and the benchmarks. Each benchmark is a Cargo workspace of its own, which `cargo xtask ci` does not build. |
| `fuzz` | Package `nomux-fuzz`: the `cargo-fuzz` targets for the terminal state, the redraw sequence, the OSC parsers, the holder messages, the program reports, the ring buffer reader and the journal reader. It is a Cargo workspace of its own with a nightly toolchain, which `cargo xtask ci` does not build. [fuzz/README.md](fuzz/README.md) describes the targets. |
| `tests` | Package `nomux-end-to-end`: tests that run the real `nomux`, `nomuxd` and `nomux-holder` together, also in a pseudoterminal, against an SSH server that a test starts, with a test extension, with a test adapter extension of the made-up tool `testtool`, with a program that reports its status, and after a stop of each process as a restart of the host does it, and the programs that measure the attach path, a remote host, the start of the daemon with dormant sessions, the request for the memory of each session and `nomux hook`. The test support stops each process that a test started and fails a test that left one. |
| `docs` | Developer notes of the core. |
| `Cargo.toml` | Workspace members, shared package fields, lint policy, release profile. |
| `deny.toml`, `clippy.toml`, `rust-toolchain.toml` | License and source policy, clippy settings, pinned toolchain. |
| `cliff.toml`, `CHANGELOG.md` | Changelog configuration and the changelog. |
| `install.sh` | Install script for the release archives. POSIX `sh`. |
| `scripts` | Release tools: the workspace version, the build of one target, the archive of one target and its check, the release notes, the test of `install.sh`. |
| `.github/workflows` | `ci.yml` (Rust checks, repository lint, actionlint, install script), `fuzz.yml` (fuzz targets: a short run for a pull request, a long run each night), `pull-request.yml` (sign-off, commit messages), `release.yml` (archives and GitHub release for a version tag) and `release-build.yml` (the static Linux archive, built when a pull request changes the dependencies or the release tools). |

## Rules for this repository

- `nomux-holder` has no async runtime. Do not give it a dependency on `tokio` or on a crate that starts a runtime. The same rule applies to the crates that the holder uses: `nomux-core`, `nomux-holder-protocol` and `nomux-term`.
- Do not write `unsafe` code. The workspace lints deny it. If a crate must use it, get agreement in an issue first, then permit it for one item with `#[expect(unsafe_code)]` and a `SAFETY` comment.
- A holder that runs is never replaced. A change to `nomux-holder-protocol` must be an addition that an older holder and a newer daemon can use together. [docs/holder.md](docs/holder.md) has the compatibility rules.
- Obey the dependency direction in [docs/architecture.md](docs/architecture.md). `nomux-core` depends on no other nomux crate. A client does not talk to a holder.
- Add a dependency to a crate only when its code uses it. `cargo machete` fails on an unused dependency.
- Add a license to `deny.toml` only when a real dependency needs it.
- Each crate uses the workspace lints (`[lints] workspace = true`) and the shared package fields. Do not turn off a lint for a full crate.
- The performance targets are in [bench/README.md](bench/README.md). Do not merge a change that breaks a target.
- The three binaries have the same version. The `--version` output has one source: `crates/nomux-core/src/version.rs`.
- Do not change a commit type or a group in `cliff.toml` without a change to the [release guide](https://github.com/nomux-sh/.github/blob/main/RELEASING.md) of the organization.

## Checks

Install the tools one time:

```sh
cargo install --locked cargo-nextest cargo-deny cargo-machete git-cliff
```

Before a push, run:

```sh
cargo xtask ci
git cliff --unreleased
```

`cargo xtask ci` runs the commands that CI runs, and stops at the first failure:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo deny check
cargo machete
```

If you changed a workflow, `install.sh` or a file in `scripts`, also run these commands. They need [actionlint](https://github.com/rhysd/actionlint) and [shellcheck](https://www.shellcheck.net):

```sh
actionlint
shellcheck install.sh scripts/*.sh
scripts/test-install.sh
```

If you changed `fuzz`, `crates/nomux-term` or `crates/nomux-holder-protocol`, also run the fuzz targets of the code that you changed. [fuzz/README.md](fuzz/README.md) has the list. A target needs `cargo-fuzz`, and rustup installs the nightly toolchain of `fuzz/rust-toolchain.toml` at the first run:

```sh
cargo install --locked cargo-fuzz@0.13.2
cargo xtask fuzz terminal_feed 60
```

The two arguments are the target and the number of seconds. If you changed `fuzz`, also run `cargo fmt --check` and `cargo clippy -- -D warnings` with `--manifest-path fuzz/Cargo.toml`.

If you changed `AGENTS.md`, also run this command with a clone of the [`.github`](https://github.com/nomux-sh/.github) repository next to this repository:

```sh
sh ../.github/standards/sync.sh . && git diff --exit-code
```

<!-- nomux:common:start -->
## Organization rules

These rules apply to each repository of the nomux-sh organization. The source of this block is `standards/AGENTS.common.md` in the [`.github`](https://github.com/nomux-sh/.github) repository. Do not edit the block in other repositories. CI compares it with the source.

### Before you change something

- Read the code around the change. Use the names, the structure and the style that the repository already has.
- Use the terms of the [glossary](https://github.com/nomux-sh/docs/blob/main/src/content/docs/reference/glossary.md). One concept has one name in code, CLI, UI and documentation.
- An architecture change needs an ADR in the [`docs`](https://github.com/nomux-sh/docs) repository before the implementation.
- Check versions, APIs and file paths before you use them. Do not guess.

### Scope

- Make only the change that the task asks for. Do not add options, abstractions or helpers that the task does not need.
- Do not add a placeholder that looks like an implementation. If work is deferred, say it in the pull request.
- Validate input at system boundaries. Trust internal code.

### Code

- Keep it simple (KISS). Build only what the task needs now (YAGNI). Have one source for each piece of logic (DRY).
- Write modern, readable code. Do not write clever code that the reader must decode.
- One type, one module and one file have one responsibility. Do not make a type that knows or does everything. Split it before it grows.
- Keep the file tree tidy. A new file goes where its neighbours have the same responsibility. Do not make catch-all directories or files such as `utils`, `common`, `helpers` or `misc`.
- Give each file, type and function a name that says what it is or does. Do not use abbreviations that the glossary does not have.
- Keep functions short and at one level of abstraction. Do not nest deeply. Return early.
- Do not duplicate code. If the same logic is necessary in a second place, move it to one place and call it.
- Comments: none by default. Write one line only for a reason that the code cannot show.
- Do not swallow errors. Do not add retries without a known cause.
- The performance targets of the project come before features. A change that breaks a target is not merged.
- Do not copy code from GPL, AGPL or Commons Clause projects.

### Tests

- Complete the code first. Write the tests last, and run them when the work is complete.
- Write the tests that matter: edge cases, behaviour that can break later, and the proof that the current behaviour is correct.
- Do not write a test only to have a test. Do not test what is obvious or what the compiler already guarantees.
- A test must fail when the behaviour that it guards is removed or moved.
- Do not mock the thing that the test examines.

### CI

- CI must be fast. Reviews and CI are the only things that slow a merge.
- Cache dependencies and build output. Run independent jobs in parallel. Cancel a run when a newer commit arrives.
- Put slow work (long fuzz runs, benchmarks, full matrix builds) in scheduled runs, not in pull request runs.
- When you add a CI step, measure the time that it adds and say it in the pull request.

### Text

- Write code, comments, documentation, commit messages and pull requests in English.
- Use short sentences, the active voice and one word for one concept. Put the condition before the instruction.
- Do not use these words: delve, robust, seamless, seamlessly, leverage, utilize, harness, pivotal, intricate, furthermore, moreover, notably, cutting-edge, best-in-class.

### Commits

- One subject line: `type: description`. No scope. 72 characters at most.
- Types: `build`, `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, `test`. Add `!` before the colon for a breaking change.
- The description says what changed, in plain English. Do not name a phase, a milestone or a ticket.
- Sign off each commit with `git commit -s`. `Signed-off-by` is the only trailer.
- Do not add `Co-authored-by`, a tool name or a "generated with" line to a commit or to a pull request.
- Enable the commit hook one time in each clone: `git config core.hooksPath .githooks`.

### Pull requests

- One logical change in each pull request. The branch name is short and names the change.
- The title has the format of a commit subject, 70 characters at most.
- The description says what changed and why, then shows the commands that you ran and their real result.
- Each commit in the pull request obeys the commit rules. The maintainers merge with a rebase, thus each commit goes to `main` unchanged.
- Do not merge when a check fails. Do not disable a check to make it pass.

### Never commit

- Secrets, tokens, keys or passwords.
- Paths, host names or account names of a local machine.
- Generated build output, editor files or local configuration.

### Before you push

1. Run the checks that the "Checks" section of this file lists.
2. Read your diff from top to bottom.
3. Make sure that the changelog and the documentation agree with the change.
<!-- nomux:common:end -->
