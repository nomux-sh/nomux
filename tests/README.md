# tests

This directory is the package `nomux-end-to-end`: the end-to-end tests of the core. An end-to-end test runs the real `nomux`, `nomuxd` and `nomux-holder` binaries together, with the `nomux` CLI as the client.

## Tests

| File | Subject |
|---|---|
| `session_lifecycle.rs` | The CLI starts the daemon and a session. The session continues after `SIGKILL` of the daemon, after `SIGKILL` of the process group of the daemon and after `nomux daemon stop`, and a new daemon finds it. A holder that does not run leaves the list. Exit codes |
| `single_writer_lease.rs` | A daemon that lost the lease of a holder does not write to it and stops |
| `version_mismatch.rs` | The CLI and a daemon without a common protocol version: exit code 5 |
| `support/lib.rs` | The library of the tests: a private home directory for each test, the CLI and the processes |

Each test uses its own home directory in `/tmp`. Thus the tests run in parallel and do not touch `~/.nomux`.

## Run

```sh
cargo nextest run --workspace
```

Cargo gives a test the path of a binary only for the binaries of the same package, and this package has no binary. Thus the tests look for the three binaries in the target directory, where a build of the workspace puts them. To run only this package, build the workspace first:

```sh
cargo build --workspace && cargo nextest run -p nomux-end-to-end
```

## Where the other tests are

- Unit tests are in the source file of the code that they test.
- The tests that run one binary are in the package of the binary: `crates/nomux-holder/tests`, `crates/nomuxd/tests` and `crates/nomux-cli/tests`.

The planned tests for attach (start a session, detach, attach again, and compare the screen) come with attach. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).
