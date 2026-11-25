# nomux

The nomux core: the session holder (`nomux-holder`), the daemon (`nomuxd`), the status engine, the transport layer, the client library, and the CLI (`nomux`).

This repository is a part of [nomux](https://github.com/nomux-sh), a native and lightweight agentic development environment. Sessions stay alive on the host when the client disconnects.

## Status

In development. Versions earlier than 1.0.0 are pre-releases: interfaces can change between minor versions.

The repository now has the Cargo workspace, the developer tools, CI and the first version of the holder ([docs/holder.md](docs/holder.md)), of the daemon ([docs/daemon.md](docs/daemon.md)) and of the CLI ([docs/cli.md](docs/cli.md)). The CLI creates, lists and ends sessions, and a session continues when the daemon stops. A client cannot attach to the terminal of a session yet. Attach and the rest of the holder are the next work: refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).

## Quick start

Build the binaries (see "Build") and put `target/release` into `PATH`. Then:

```console
$ nomux new --cmd sh          # starts the daemon and a session, prints the session identifier
k3xq2abc
$ nomux ls
ID        STATE    PID
k3xq2abc  running  41873
$ nomux daemon stop           # the session continues without the daemon
nomuxd stopped
$ nomux ls                    # starts a new daemon, which finds the session
ID        STATE    PID
k3xq2abc  running  41873
$ nomux kill k3xq2abc
session k3xq2abc ended
```

Each command accepts `--json`. [docs/cli.md](docs/cli.md) has the JSON schema and the exit codes.

## Install

No release exists yet. The commands in this section work from the first release.

Each release has one archive for each target and a `SHA256SUMS` file. An archive contains `nomux`, `nomuxd`, `nomux-holder`, this file and the two license files.

| Host | Target |
|---|---|
| Linux, x86_64 | `x86_64-unknown-linux-musl` |
| Linux, aarch64 | `aarch64-unknown-linux-musl` |
| macOS, Intel | `x86_64-apple-darwin` |
| macOS, Apple silicon | `aarch64-apple-darwin` |

The Linux binaries are static. They do not use the C library of the host.

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
| `crates/nomux-core` | library | Shared types, errors, configuration | in use: the version output of the binaries, the lookup of a binary of the same installation |
| `crates/nomux-term` | library | Thin abstraction over the VT library (`alacritty_terminal`) | in use: the screen state, the redraw sequence and the OSC sequences |
| `crates/nomux-holder-protocol` | library | Contract between a holder and the daemon: control messages, ring buffer layout, discovery files | in use by the holder |
| `crates/nomux-holder` | binary `nomux-holder` | Session holder: owns the PTY of one session. No async runtime | first version: PTY, ring buffer, control socket, discovery files |
| `crates/nomux-transport` | library | Unix socket, SSH channels, QUIC streams, credit windows | first version: the Unix socket of the local host |
| `crates/nomux-status` | library | Agent status resolver, rules, aggregation | structure only |
| `crates/nomux-ext-host` | library | Extension host of the daemon | structure only |
| `crates/nomuxd` | binary `nomuxd` | Daemon | first version: creates, lists and ends sessions, finds the holders after a restart |
| `crates/nomux-client` | library | Protocol client for the CLI, the desktop client and the mobile clients | first version: connection, handshake, requests for sessions and for the daemon |
| `crates/nomux-cli` | binary `nomux` | CLI | first version: `new`, `ls`, `kill`, `daemon status`, `daemon stop`, `version` |
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
