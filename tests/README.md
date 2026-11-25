# tests

This directory is the package `nomux-end-to-end`: the end-to-end tests of the core. An end-to-end test runs the real `nomux`, `nomuxd` and `nomux-holder` binaries together. The client is the `nomux` CLI, or the client library where a test needs an exact sequence of requests.

## Tests

| File | Subject |
|---|---|
| `attach.rs` | `nomux attach` in a pseudoterminal: the screen of a session that already drew, the keys and their echo, the window size, detach and a second attach with the same screen, two clients, `nomux detach`, `SIGKILL` of the daemon during an attach, a flood of output for a client that takes no output, the exit status of the session, `new --attach`, and the command after `--` |
| `extensions.rs` | The `nomux ext` commands and the extension host of the daemon, with the test extension of `nomux-ext-host`: an install without an approval installs nothing, the first call starts the process and the idle process stops, the `sessions.list` service with and without its scope, a process that fails and starts again, an extension that the daemon disables after failures, an attach to a session while a call of an extension that hangs waits, the stop of the daemon, `ext rm`, and `ext validate` |
| `attach_protocol.rs` | The attach of the client library: a client that attaches again gets only the output that it does not have, also after a restart of the daemon, and the screen when the daemon no longer has that output. The errors of `Session.Resize` and `Session.Input` |
| `remote_host.rs` | A remote host through SSH, with an `sshd` that each test starts: `host add` installs the binaries from a release archive, the session commands and an attach with `--host`, an attach that continues after the connection was cut with each byte one time, a host key that is not known or that changed, a daemon without a common protocol version that is replaced while its session continues, `host doctor` for a good host and for a wrong permission, ssh-agent, `ProxyJump`, and a server that refuses connections to Unix sockets |
| `session_lifecycle.rs` | The CLI starts the daemon and a session. The session continues after `SIGKILL` of the daemon, after `SIGKILL` of the process group of the daemon, after `nomux daemon stop` and after `nomuxd --replace`, and a new daemon finds it. A holder that does not run leaves the list. Exit codes |
| `single_writer_lease.rs` | A daemon that lost the lease of a holder does not write to it and stops |
| `version_mismatch.rs` | The CLI and a daemon without a common protocol version: exit code 5 |
| `support/lib.rs` | The library of the tests: a private home directory for each test, the CLI and the processes |
| `support/terminal_client.rs` | A `nomux` process in a pseudoterminal: the test types keys and reads the screen. `nomux-term` makes the screen from the output of the process |
| `support/remote_host.rs` | A client and a remote host for a test: two home directories, a key, the file of known hosts, and a release archive of the binaries of the build |
| `support/ssh_server.rs` | An `sshd` that runs as the user of the tests on a free port of this computer, with a host key and an authorized key of its own |
| `support/tcp_relay.rs` | A TCP relay between the client and the `sshd`: a test cuts its connections, and a measurement gives it a delay |
| `support/extension_package.rs` | A package of the test extension: a manifest and the binary `nomux-test-extension` of the package `nomux-ext-host` |
| `support/incompatible_daemon.rs` | The binary `nomux-test-incompatible-daemon`: a process in the place of a daemon that refuses each client as a daemon without a common protocol version does |
| `support/key_latency.rs` | The measurement of the time from a key to its echo |
| `measurements/attach.rs` | Not a test: the program that measures the attach path |
| `measurements/remote.rs` | Not a test: the program that measures a remote host |

Each test uses its own home directory in `/tmp`. Thus the tests run in parallel and do not touch `~/.nomux`.

### The SSH server of the tests

A test of `remote_host.rs` starts `/usr/sbin/sshd` (or `/usr/bin/sshd`) as the user who runs the tests. No `root` user and no change of the computer is necessary.

- The server listens on a free port of `127.0.0.1`, with a new host key, and it accepts one new key. The files are in a temporary directory.
- `SetEnv HOME=<directory>` gives each command of the server a temporary home directory. Thus the "remote" nomux uses that directory and not `~/.nomux`.
- The client has a second temporary home directory with `~/.ssh` and the configuration file of nomux.
- The tests need `sshd`, `ssh-keygen`, `ssh-agent` and `ssh-add` of OpenSSH 7.8 or later. If the computer has no `sshd`, each of these tests writes `SKIPPED` with the reason to the standard error and passes. `cargo nextest run --no-capture` shows the line.

## Run

```sh
cargo nextest run --workspace
```

Cargo gives a test the path of a binary only for the binaries of the same package, and this package has no binary. Thus the tests look for the three binaries and for the test extension in the target directory, where a build of the workspace puts them. To run only this package, build the workspace first:

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

`measurements/remote.rs` measures a remote host with the same SSH server as the tests: the time of the first `nomux host add` with the upload of the binaries, the time of a later command, and the time from a key to its echo with `--host`, without a delay, with a round trip of 100 ms, and with a model of lost packets. [docs/remote.md](../docs/remote.md) has the results and the limits of the model.

```sh
cargo build --release --workspace
cargo run --release -p nomux-end-to-end --example remote_measurements
```

A run takes approximately 2 minutes.
