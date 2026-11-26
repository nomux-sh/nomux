# nomux

The nomux core: the session holder (`nomux-holder`), the daemon (`nomuxd`), the status engine, the transport layer, the client library, and the CLI (`nomux`).

This repository is a part of [nomux](https://github.com/nomux-sh), a native and lightweight agentic development environment. Sessions stay alive on the host when the client disconnects.

## Status

In development. Versions earlier than 1.0.0 are pre-releases: interfaces can change between minor versions.

The repository now has the Cargo workspace, the developer tools, CI and the first version of the holder ([docs/holder.md](docs/holder.md)), of the daemon ([docs/daemon.md](docs/daemon.md)) and of the CLI ([docs/cli.md](docs/cli.md)). The CLI creates, lists and ends sessions, attaches a terminal to a session and detaches it, and a session continues when the daemon stops. Each session keeps its output in a journal on disk: a client reads the scrollback from it, and after a restart of the host a session starts again when it is opened. The same commands work on a remote host through SSH ([docs/remote.md](docs/remote.md)). The daemon runs extensions that the user installs from a directory ([docs/extensions.md](docs/extensions.md)). `nomux status` shows what runs in each session and what waits for the user, with the source of each status ([docs/status.md](docs/status.md)). `nomux project add` and `nomux group new` keep the Projects of a host in Groups, and `nomux status --tree` shows the status of each Group ([docs/workspace.md](docs/workspace.md)). Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md) for the next work.

## Quick start

1. Install the three binaries: see "Install".

2. Start a session and attach to it. The first command starts the daemon.

   ```console
   $ nomux new
   k3xq2abc
   $ nomux attach k3xq2abc
   ```

   `nomux new --attach` does the two steps with one command. `nomux new -- vim notes.txt` starts a session with a command that is not the shell.

3. The terminal now shows the session. To detach, press `Ctrl-\` and then `d`. The session continues, and its programs do not see the detach.

   ```console
   detached from session k3xq2abc
   $ nomux ls
   ID        STATE    PID    COMMAND
   k3xq2abc  running  41873  /bin/zsh
   ```

4. Attach again from the same terminal or from a different one. More than one terminal can show the same session at the same time.

   ```console
   $ nomux attach k3xq2abc
   ```

5. End the session:

   ```console
   $ nomux kill k3xq2abc
   session k3xq2abc ended
   ```

A session does not need the daemon: it continues when the daemon stops (`nomux daemon stop`), fails or is updated. The next command starts a new daemon, which finds the session.

`nomux capture k3xq2abc --lines 50` prints the last 50 lines that the session wrote, also the lines that left its screen.

### After a restart of the host

A restart of the host stops each process, thus also the commands of the sessions. The sessions are not lost: each one keeps its command, its working directory, its window size and its output on disk.

```console
$ nomux ls
ID        STATE    PID  COMMAND
k3xq2abc  dormant  -    /bin/zsh
m7pd4xyz  dormant  -    npm run dev
```

- A dormant session uses no process and no memory. `nomux attach k3xq2abc` starts its command again in its working directory. The terminal shows the end of the old output, a line that says that the session started again, and the new command. The programs that ran before the restart do not come back.
- `nomux capture k3xq2abc` prints the old output without a start of the session.
- `nomux restore-policy m7pd4xyz auto` makes a session start again when the daemon starts, for example a development server. The other policies are `on-focus` (the default: the session starts at the first attach) and `manual` (only `nomux start`).
- `nomux daemon install-service` writes the file that starts the daemon at login: a systemd user unit on Linux, a launchd agent on macOS. Without it, the first `nomux` command after the restart starts the daemon.
- `nomux kill k3xq2abc` removes a dormant session and its output.

A session whose command ended is not dormant. The daemon keeps it and its output for 24 hours: `nomux ls --all` shows it with its exit status.

Each command accepts `--json`. [docs/cli.md](docs/cli.md) has the commands, the keys of an attach, the JSON schema and the exit codes.

### A session on a remote host

The remote host needs an SSH server and a key of the user in `authorized_keys`. It needs no nomux package: nomux installs itself there.

1. Make sure that `ssh` knows the key of the host. If you did not connect to the host before, do it one time and compare the fingerprint that `ssh` shows:

   ```console
   $ ssh deploy@vps.example.com exit
   ```

2. Add the host. The command installs the three binaries in `~/.nomux/bin` of the host, starts the daemon there and saves the host with the name `vps`:

   ```console
   $ nomux host add deploy@vps.example.com --name vps
   host vps added: Linux x86_64, nomux in ~/.nomux/bin/0.1.0 (installed), nomuxd 0.1.0 runs: process ID 2210, 0 sessions
   ```

3. Use each command with `--host vps`:

   ```console
   $ nomux --host vps new --attach
   $ nomux --host vps ls
   ```

The session continues on the host when the local computer disconnects, and `nomux --host vps attach <id>` shows the same screen again. An attach that loses its network connection connects again and continues without lost output.

nomux uses `~/.ssh/config`, ssh-agent and the key files of the user. It asks for no password and stores none. `nomux host doctor vps` checks a host. [docs/remote.md](docs/remote.md) has the details, also for a build that has no release.

### Extensions

An extension is a package in a directory: a manifest (`extension.toml`) and the program of its daemon part. The [`extension-api`](https://github.com/nomux-sh/extension-api) repository has the SDK and an example. `nomux ext install` also takes a release archive (`.tar.gz`), an `https` address of one, or the short form of an extension of [`nomux-sh/extensions`](https://github.com/nomux-sh/extensions), for example `nomux ext install git`: it downloads the archive for the host with `curl`, compares its checksum and shows the extension before it writes anything. [docs/cli.md](docs/cli.md#from-a-release-archive) has the rules.

1. Install the extension. Without an approval, the command prints the permissions that the extension asks for and installs nothing:

   ```console
   $ nomux ext install ./nomux-git
   nomux: the extension nomux.git 1.0.0 asks for these permissions:
     process=git
     scope=sessions:read
   Nothing was installed. To install the extension, approve its permissions: add --grant-all, or --grant <permission> for each permission that you approve. For a directory that starts with ${worktree}, give a directory: --grant fs=<directory>
   $ nomux ext install ./nomux-git --grant-all
   extension nomux.git 1.0.0 installed with the permissions: process=git scope=sessions:read
   ```

2. Call a method. The first call starts the process of the extension, and the daemon stops the process after 60 seconds without a call. The payload and the result are bytes in the encoding of the method:

   ```console
   $ nomux ext call nomux.git status --payload @request.bin > response.bin
   $ nomux ext ls
   ID         NAME  VERSION  STATE    PERMISSIONS                      REASON
   nomux.git  Git   1.0.0    running  process=git,scope=sessions:read
   ```

An extension runs as a separate process. If it fails, the caller gets an error, and the daemon and the sessions continue. `nomux ext rm nomux.git` removes it, and `nomux ext validate <directory>` checks a manifest without a daemon. [docs/cli.md](docs/cli.md) has the `ext` commands, and [docs/extensions.md](docs/extensions.md) describes the extension host and its limits.

### What waits for you

A tool in a session reports its status in the terminal stream (OSC 7501), as Claude Code, Codex and Qwen Code do, or with `nomux status set`. `nomux status` shows each session:

```console
$ nomux status
  SESSION   COMMAND  STATE                  FOR  MESSAGE
! k3xq2abc  claude   needs you: permission  2m   run tests?
* p7wz4def  make     working (guess)        14s
$ nomux agent explain k3xq2abc
```

A guess of the daemon says `(guess)`: a guess is never shown as a fact. `nomux status --tree` also shows the subagents, `nomux events --follow` prints each change, and an attach marks a result as seen. A script in a session reports with `nomux status set working "tests run"`.

An adapter extension of a tool gets the events of its hooks: the hook runs `nomux hook <tool> <event>`. `nomux hooks install <tool>` prints the change that the hooks need in the settings file of the tool and writes it only with `--apply`, and `nomux hooks uninstall <tool> --apply` gives the file its bytes back. [docs/adapters.md](docs/adapters.md) describes the adapters.

## Install

Each release has one archive for each target and a `SHA256SUMS` file. An archive contains `nomux`, `nomuxd`, `nomux-holder`, this file and the two license files.

| Host | Target |
|---|---|
| Linux, x86_64 | `x86_64-unknown-linux-musl` |
| Linux, aarch64 | `aarch64-unknown-linux-musl` |
| macOS, Intel | `x86_64-apple-darwin` |
| macOS, Apple silicon | `aarch64-apple-darwin` |

The Linux binaries are static. They do not use the C library of the host.

The word `unknown` in the names of the Linux targets is the vendor field of the Rust target name. It is the same for all Linux targets and does not mean that a part of the target is not known.

### With the install script

The script finds the target of the host, downloads the archive, verifies its SHA-256 checksum and puts the three binaries into `~/.local/bin`. It does not use `sudo`.

```sh
curl -fsSL https://raw.githubusercontent.com/nomux-sh/nomux/main/install.sh | sh
```

Without options, the script installs the newest version that has no suffix. To select a version or a directory, add the options:

```sh
curl -fsSL https://raw.githubusercontent.com/nomux-sh/nomux/main/install.sh | sh -s -- --version 0.1.0 --dir ~/bin
```

### With a manual download

1. Set the version and the target of the host:

   ```sh
   version=0.1.0
   target=x86_64-unknown-linux-musl
   ```

2. Download the archive and the checksums from the [releases](https://github.com/nomux-sh/nomux/releases):

   ```sh
   curl -fsSLO "https://github.com/nomux-sh/nomux/releases/download/v$version/nomux-$version-$target.tar.gz"
   curl -fsSLO "https://github.com/nomux-sh/nomux/releases/download/v$version/SHA256SUMS"
   ```

3. Verify the checksum. The command must print `OK`. On macOS, use `shasum -a 256 --check` as the last command.

   ```sh
   grep "nomux-$version-$target.tar.gz" SHA256SUMS | sha256sum --check
   ```

4. Extract the archive and copy the binaries to a directory that is in `PATH`:

   ```sh
   tar -xzf "nomux-$version-$target.tar.gz"
   mkdir -p ~/.local/bin
   cp "nomux-$version-$target/nomux" "nomux-$version-$target/nomuxd" "nomux-$version-$target/nomux-holder" ~/.local/bin/
   nomux --version
   ```

The binaries are not signed and not notarized. Code signing and notarization start with the first stable release.

## Layout

| Path | Type | Contents | State |
|---|---|---|---|
| `crates/nomux-core` | library | Shared types, errors, configuration | in use: the version output of the binaries, the lookup of a binary of the same installation, the hosts of the global configuration file |
| `crates/nomux-term` | library | Thin abstraction over the VT library (`alacritty_terminal`) | in use by the holder: the screen state, the redraw sequence and the OSC sequences |
| `crates/nomux-holder-protocol` | library | Contract between a holder and the daemon: control messages, ring buffer layout, redraw memory, discovery files | in use by the holder |
| `crates/nomux-holder` | binary `nomux-holder` | Session holder: owns the PTY of one session. No async runtime | first version: PTY, ring buffer, screen state, control socket, discovery files |
| `crates/nomux-transport` | library | Unix socket, SSH channels, QUIC streams, credit windows | first version: the Unix socket of the local host, the SSH client for a remote host, the control channel and the terminal channels of an attach |
| `crates/nomux-status` | library | Agent status resolver, rules, aggregation | in use by the daemon: the status model, the source sequence, the Child work tree, the aggregation and the rule for a session without a process |
| `crates/nomux-ext-host` | library | Extension host of the daemon | first version: installed extensions, a process for each extension in use, calls, events, isolation of failures. The daemon does not use it yet |
| `crates/nomuxd` | binary `nomuxd` | Daemon | first version: creates, lists and ends sessions, finds the holders after a restart, attaches clients to a session |
| `crates/nomux-client` | library | Protocol client for the CLI, the desktop client and the mobile clients | first version: the local host and remote hosts, the installation on a remote host, handshake, requests for sessions and for the daemon, attach with reconnect |
| `crates/nomux-cli` | binary `nomux` | CLI | first version: `new`, `ls`, `attach`, `detach`, `kill`, `daemon status`, `daemon stop`, `host add`, `host ls`, `host rm`, `host doctor`, `version`, and `--host` |
| `xtask` | binary | Developer tasks: `cargo xtask` | in use |
| `bench` | directory | Performance tests and the performance targets | in use: the VT library benchmark |
| `fuzz` | directory | Fuzz targets | structure only |
| `tests` | package `nomux-end-to-end` | End-to-end tests with the three binaries | in use |
| `docs` | directory | Developer notes. Start with [docs/architecture.md](docs/architecture.md) | in use |

## Build

The supported hosts are Linux and macOS.

1. Install [rustup](https://rustup.rs). `rust-toolchain.toml` pins the Rust version, and rustup installs it automatically.
2. Build the workspace:

   ```sh
   cargo build --release
   ```

The binaries are in `target/release/`: `nomux`, `nomuxd` and `nomux-holder`.

```sh
target/release/nomux --version
```

## Checks

Install the tools one time:

```sh
cargo install --locked cargo-nextest cargo-deny cargo-machete
```

Run the checks with one command:

```sh
cargo xtask ci
```

`cargo xtask ci` runs the commands that CI runs, and stops at the first failure:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test --workspace --doc
cargo deny check     # licenses, security advisories, sources
cargo machete        # dependencies that no code uses
```

The lint policy is in the `[workspace.lints]` section of `Cargo.toml`. The license policy is in `deny.toml`.

## Release

A version tag `vX.Y.Z` on `main` starts [`.github/workflows/release.yml`](.github/workflows/release.yml). The workflow builds the archives and makes the GitHub release. The [release guide](https://github.com/nomux-sh/.github/blob/main/RELEASING.md) of the organization has the steps.

To build the archives without a release, start the workflow manually with `dry-run`:

```sh
gh workflow run release.yml -f dry-run=true
```

## Contributing

Read the [contribution guide](https://github.com/nomux-sh/.github/blob/main/CONTRIBUTING.md) of the organization and [AGENTS.md](AGENTS.md), which has the rules for this repository.

- A commit message is one subject line, `type: description`, and the sign-off (`git commit -s`).
- Enable the commit hook one time in each clone: `git config core.hooksPath .githooks`.
- [git-cliff](https://git-cliff.org) writes `CHANGELOG.md` from the commit subjects. The [release guide](https://github.com/nomux-sh/.github/blob/main/RELEASING.md) has the steps.

## Security

Do not report a vulnerability in a public issue. Obey the [security policy](https://github.com/nomux-sh/.github/blob/main/SECURITY.md) of the organization.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

The documents in `docs/` are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
