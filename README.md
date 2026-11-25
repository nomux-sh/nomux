# nomux

The nomux core: the session holder (`nomux-holder`), the daemon (`nomuxd`), the status engine, the transport layer, the client library, and the CLI (`nomux`).

This repository is a part of [nomux](https://github.com/nomux-sh), a native and lightweight agentic development environment. Sessions stay alive on the host when the client disconnects.

## Status

In development. Versions earlier than 1.0.0 are pre-releases: interfaces can change between minor versions.

The repository now has the Cargo workspace, the developer tools, CI and the first version of the holder ([docs/holder.md](docs/holder.md)). The daemon and the CLI print their version and do nothing else. The daemon, the rest of the holder and the CLI commands are the next work: refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).

## Layout

| Path | Type | Contents | State |
|---|---|---|---|
| `crates/nomux-core` | library | Shared types, errors, configuration | in use: the version output of the binaries |
| `crates/nomux-term` | library | Thin abstraction over the VT library | structure only |
| `crates/nomux-holder-protocol` | library | Contract between a holder and the daemon: control messages, ring buffer layout, discovery files | in use by the holder |
| `crates/nomux-holder` | binary `nomux-holder` | Session holder: owns the PTY of one session. No async runtime | first version: PTY, ring buffer, control socket, discovery files |
| `crates/nomux-transport` | library | Unix socket, SSH channels, QUIC streams, credit windows | structure only |
| `crates/nomux-status` | library | Agent status resolver, rules, aggregation | structure only |
| `crates/nomux-ext-host` | library | Extension host of the daemon | structure only |
| `crates/nomuxd` | binary `nomuxd` | Daemon | prints its version only |
| `crates/nomux-client` | library | Protocol client for the CLI, the desktop client and the mobile clients | structure only |
| `crates/nomux-cli` | binary `nomux` | CLI | prints its version only |
| `xtask` | binary | Developer tasks: `cargo xtask` | in use |
| `bench` | directory | Performance tests and the performance targets | structure only |
| `fuzz` | directory | Fuzz targets | structure only |
| `tests` | directory | End-to-end tests | structure only |
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
