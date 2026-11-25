# AGENTS.md

Rules for coding agents and for people who work in this repository. `CLAUDE.md` and `QWEN.md` refer to this file.

## This repository

`nomux-sh/nomux` is the nomux core: the session holder (`nomux-holder`), the daemon (`nomuxd`), the CLI (`nomux`) and the libraries that they share. It is one Cargo workspace. [docs/architecture.md](docs/architecture.md) shows the components.

| Path | Responsibility |
|---|---|
| `crates/nomux-core` | Types that more than one crate uses. Now: the `--version` output of the three binaries. |
| `crates/nomux-term` | Abstraction over the VT library. |
| `crates/nomux-holder` | Binary `nomux-holder`: owns the PTY of one session. |
| `crates/nomux-transport` | Unix socket, SSH channels, QUIC streams, credit windows. |
| `crates/nomux-status` | Agent status resolver, rules, aggregation. |
| `crates/nomux-ext-host` | Extension host of the daemon. |
| `crates/nomuxd` | Binary `nomuxd`: the daemon. |
| `crates/nomux-client` | Protocol client for the CLI, the desktop client and the mobile clients. |
| `crates/nomux-cli` | Binary `nomux`: the CLI. |
| `xtask` | Developer tasks. `cargo xtask ci` runs the checks. |
| `bench`, `fuzz`, `tests` | Performance targets, fuzz targets and end-to-end tests. Each has a `README.md` only. |
| `docs` | Developer notes of the core. |
| `Cargo.toml` | Workspace members, shared package fields, lint policy, release profile. |
| `deny.toml`, `clippy.toml`, `rust-toolchain.toml` | License and source policy, clippy settings, pinned toolchain. |
| `cliff.toml`, `CHANGELOG.md` | Changelog configuration and the changelog. |
| `install.sh` | Install script for the release archives. POSIX `sh`. |
| `scripts` | Release tools: the workspace version, the archive of one target and its check, the release notes, the test of `install.sh`. |
| `.github/workflows` | `ci.yml` (Rust checks, repository lint, actionlint, install script), `pull-request.yml` (sign-off, commit messages) and `release.yml` (archives and GitHub release for a version tag). |

## Rules for this repository

- `nomux-holder` has no async runtime. Do not give it a dependency on `tokio` or on a crate that starts a runtime. The same rule applies to the crates that the holder uses: `nomux-core` and `nomux-term`.
- Do not write `unsafe` code. The workspace lints deny it. If a crate must use it, get agreement in an issue first, then permit it for one item with `#[expect(unsafe_code)]` and a `SAFETY` comment.
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
