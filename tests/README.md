# tests

This directory is the package `nomux-end-to-end`: the end-to-end tests of the core. An end-to-end test runs the real `nomux`, `nomuxd` and `nomux-holder` binaries together. The client is the `nomux` CLI, or the client library where a test needs an exact sequence of requests.

## Tests

| File | Subject |
|---|---|
| `attach.rs` | `nomux attach` in a pseudoterminal: the screen of a session that already drew, the keys and their echo, the window size, detach and a second attach with the same screen, two clients, `nomux detach`, `SIGKILL` of the daemon during an attach, a flood of output for a client that takes no output, the exit status of the session, `new --attach`, and the command after `--` |
| `attach_protocol.rs` | The attach of the client library: a client that attaches again gets only the output that it does not have, also after a restart of the daemon, and the screen when the daemon no longer has that output. The errors of `Session.Resize` and `Session.Input` |
| `session_lifecycle.rs` | The CLI starts the daemon and a session. The session continues after `SIGKILL` of the daemon, after `SIGKILL` of the process group of the daemon and after `nomux daemon stop`, and a new daemon finds it. A holder that does not run leaves the list. Exit codes |
| `single_writer_lease.rs` | A daemon that lost the lease of a holder does not write to it and stops |
| `version_mismatch.rs` | The CLI and a daemon without a common protocol version: exit code 5 |
| `support/lib.rs` | The library of the tests: a private home directory for each test, the CLI and the processes |
| `support/terminal_client.rs` | A `nomux` process in a pseudoterminal: the test types keys and reads the screen. `nomux-term` makes the screen from the output of the process |
| `measurements/attach.rs` | Not a test: the program that measures the attach path |

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

## Measurements

`measurements/attach.rs` measures the time from a key to its echo on the screen of `nomux attach`, without and with a flood of output in a second session, the resident memory of the daemon with one attached client, and the sizes of the binaries. It prints Markdown tables. [docs/daemon.md](../docs/daemon.md) has the results.

```sh
cargo build --release --workspace
cargo run --release -p nomux-end-to-end --example attach_measurements
```

A run takes approximately 20 seconds. Run it on a computer that does no other work: the largest value of each measurement comes from the scheduler.
