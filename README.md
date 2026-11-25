# nomux

The nomux core: the session holder (`nomux-holder`), the daemon (`nomuxd`), the status engine, the transport layer, the client library, and the CLI (`nomux`).

This repository is a part of [nomux](https://github.com/nomux-sh), a native and lightweight agentic development environment. Sessions stay alive on the host when the client disconnects.

## Status

In development. There is no public release.

The current milestone is M0: the workspace, the tools and CI.
The three binaries print their version for `--version` and do nothing else.
Milestone M1 adds the holder, the daemon and the first CLI commands.

## Layout

| Path | Type | Contents | First code |
|---|---|---|---|
| `crates/nomux-core` | library | Shared types, errors, configuration | M0 (build information), M1 |
| `crates/nomux-term` | library | Thin abstraction over the VT library | M1 |
| `crates/nomux-holder` | binary `nomux-holder` | Session holder: owns the PTY of one session. No async runtime | M1 |
| `crates/nomux-transport` | library | Unix socket, SSH channels, QUIC streams, credit windows | M1, M2, M8 |
| `crates/nomux-status` | library | Agent status resolver, rules, aggregation | M4 |
| `crates/nomux-ext-host` | library | Extension host of the daemon | M3 |
| `crates/nomuxd` | binary `nomuxd` | Daemon | M1 |
| `crates/nomux-client` | library | Protocol client for the CLI, the desktop client and the mobile clients | M1 |
| `crates/nomux-cli` | binary `nomux` | CLI | M1 |
| `xtask` | binary | Developer tasks: `cargo xtask` | M0 |
| `bench` | directory | Performance tests and the performance targets | M1 |
| `fuzz` | directory | Fuzz targets | M1 |
| `tests` | directory | End-to-end tests | M1 |
| `docs` | directory | Developer notes. Start with [docs/architecture.md](docs/architecture.md) | M0 |

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

Run the format, lint and test checks with one command:

```sh
cargo xtask ci
```

`cargo xtask ci` runs these commands and stops at the first failure:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

CI also runs these two checks. Run them before you add or remove a dependency:

```sh
cargo deny check     # licenses, security advisories, sources
cargo machete        # dependencies that no code uses
```

The lint policy is in the `[workspace.lints]` section of `Cargo.toml`. The license policy is in `deny.toml`.

## Contributing

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).
- Sign off each commit (`git commit -s`). The sign-off states that you agree to the [Developer Certificate of Origin](https://developercertificate.org).
- `CHANGELOG.md` lists the changes. The release process generates it with [git-cliff](https://git-cliff.org) and `cliff.toml`.

## Security

Do not report a vulnerability in a public issue. Obey the [security policy](https://github.com/nomux-sh/.github/blob/main/SECURITY.md) of the organization.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

The documents in `docs/` are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
