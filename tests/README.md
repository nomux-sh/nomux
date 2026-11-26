# tests

This directory is the package `nomux-end-to-end`: the end-to-end tests of the core. An end-to-end test runs the real `nomux`, `nomuxd` and `nomux-holder` binaries together. The client is the `nomux` CLI, or the client library where a test needs an exact sequence of requests.

## Tests

| File | Subject |
|---|---|
| `attach.rs` | `nomux attach` in a pseudoterminal: the screen of a session that already drew, the keys and their echo, the window size, detach and a second attach with the same screen, two clients, `nomux detach`, `SIGKILL` of the daemon during an attach, a flood of output for a client that takes no output, the exit status of the session, `new --attach`, and the command after `--` |
| `extensions.rs` | The `nomux ext` commands and the extension host of the daemon, with the test extension of `nomux-ext-host`: an install without an approval installs nothing, the first call starts the process and the idle process stops, the `sessions.list` service with and without its scope, a process that fails and starts again, an extension that the daemon disables after failures, an attach to a session while a call of an extension that hangs waits, the stop of the daemon, `ext rm`, and `ext validate` |
| `attach_protocol.rs` | The attach of the client library: a client that attaches again gets only the output that it does not have, also after a restart of the daemon, and the screen when the daemon no longer has that output. The errors of `Session.Resize` and `Session.Input` |
| `daemon_service.rs` | `nomux daemon install-service` and `uninstall-service`: the service file is written and removed only in the configuration directory of the user, a dry run changes nothing, and no daemon starts |
| `journal.rs` | The journal through the daemon: a client that attaches again with a position that left the ring buffer gets each missing byte one time and then the output that the command writes, a client learns that output before the oldest journal segment is gone and gets the oldest screen and the output after it, and `nomux capture` gives the last lines also when they left the ring buffer |
| `process_cleanup.rs` | The test support itself: a sandbox stops the sessions and the daemon of its test, a process that a test left is stopped and fails the test, and a guarded process ends with each process of its group |
| `remote_host.rs` | A remote host through SSH, with an `sshd` that each test starts: `host add` installs the binaries from a release archive, the session commands and an attach with `--host`, an attach that continues after the connection was cut with each byte one time, a host key that is not known or that changed, a daemon without a common protocol version that is replaced while its session continues, `host doctor` for a good host and for a wrong permission, ssh-agent, `ProxyJump`, and a server that refuses connections to Unix sockets |
| `restore.rs` | Sessions without a process. After `SIGKILL` for each holder and the daemon, as a restart of the host does it, a new daemon lists the sessions as dormant with their description and starts no process; an attach shows the old output, the mark and a new shell in the directory of the session; the policy `auto` starts a session at the start of the daemon and `manual` only with `nomux start`. An ended session leaves with its state directory after the retention time, also across a restart of the daemon. A damaged `session` file, a damaged journal segment and a FIFO in the place of a file give a listed session with the reason and a daemon that works |
| `session_memory.rs` | `nomux ls --memory`: the memory of a command that allocates a known amount and of the child that it starts, the holder as a separate value, and a session without a process that has no values and is not in the total |
| `session_lifecycle.rs` | The CLI starts the daemon and a session. The session continues after `SIGKILL` of the daemon, after `SIGKILL` of the process group of the daemon, after `nomux daemon stop` and after `nomuxd --replace`, and a new daemon finds it. A holder that does not run leaves the runtime directory, and its session is dormant. Exit codes, and the usage error for `--cmd` together with `--` |
| `status.rs` | The status of the sessions: a program that answers the OSC 7501 query reports working, a Child work, blocked with the kind permission and done, each with the source `program_status`; a second session that waits longer decides the aggregate; `agent explain` names each source; `events --follow` prints the changes in their sequence and ends cleanly on `SIGINT`; an attach marks the result as seen; a new daemon gets the status from the holder again. A session whose holder ends shows its waiting work as done with the source `session_restore`, and after a restore the status of the new process. `nomux status set` works inside a session and gives exit code 4 with a wrong or a missing token, and no file and no output has the token |
| `single_writer_lease.rs` | A daemon that lost the lease of a holder does not write to it and stops |
| `version_mismatch.rs` | The CLI and a daemon without a common protocol version: exit code 5 |
| `support/lib.rs` | The library of the tests: a private home directory for each test, the CLI and the processes |
| `support/process_guard.rs` | A process that a test starts directly, in a process group of its own. Each process of the group ends with the test, also when an assertion fails |
| `support/leftover_processes.rs` | The check at the end of each sandbox: finds the processes that still have the home directory of the sandbox, stops them and reports them |
| `support/memory_user.rs` | The binary `nomux-test-memory-user`: a process that uses a number of mebibytes and starts a child that uses a second number |
| `support/terminal_client.rs` | A `nomux` process in a pseudoterminal: the test types keys and reads the screen. `nomux-term` makes the screen from the output of the process |
| `support/remote_host.rs` | A client and a remote host for a test: two home directories, a key, the file of known hosts, and a release archive of the binaries of the build |
| `support/ssh_server.rs` | An `sshd` that runs as the user of the tests on a free port of this computer, with a host key and an authorized key of its own |
| `support/tcp_relay.rs` | A TCP relay between the client and the `sshd`: a test cuts its connections, and a measurement gives it a delay |
| `support/extension_package.rs` | A package of the test extension: a manifest and the binary `nomux-test-extension` of the package `nomux-ext-host` |
| `support/incompatible_daemon.rs` | The binary `nomux-test-incompatible-daemon`: a process in the place of a daemon that refuses each client as a daemon without a common protocol version does |
| `support/status_program.rs` | The binary `nomux-test-status-program`: a command of a session that asks the OSC 7501 query and then writes one report for each line of its input |
| `support/key_latency.rs` | The measurement of the time from a key to its echo |
| `measurements/attach.rs` | Not a test: the program that measures the attach path |
| `measurements/memory.rs` | Not a test: the program that measures the request for the memory of each session |
| `measurements/remote.rs` | Not a test: the program that measures a remote host |
| `measurements/restore.rs` | Not a test: the program that measures the start of the daemon with dormant sessions that each have a full journal |

Each test uses its own home directory in `/tmp`. Thus the tests run in parallel and do not touch `~/.nomux`.

### No process stays after a test

A test must leave no process: a daemon or a holder that stays uses memory and keeps a runtime directory busy.

- A test starts a process only through the sandbox (`Sandbox::nomux`), through a `ProcessGuard`, or with the home directory of its sandbox in the `HOME` variable. The sandbox finds a process by that directory: the daemon, the holders and the commands of the sessions get it from their starter.
- A `ProcessGuard` ends each process of its process group when it is dropped, also when an assertion of the test failed before.
- When a sandbox is dropped, it stops its daemon and its holders. Then it waits 2 seconds at most for the other processes to end, stops each process that still has its home directory with `SIGKILL`, and **fails the test** with the list of these processes. During a failure of the test, it stops the processes and writes the list to the standard error.
- Thus a process that a test left fails that test in the test run, on each computer. CI needs no step for it.

The 2 seconds are for processes that end by themselves a moment after their starter: a daemon that got `SIGTERM`, and a daemon that waits for the lock of a daemon that just stopped.

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

`measurements/memory.rs` measures the request for the memory of each session: the time of one `Session.Memory` request and the time of the `nomux ls --memory --json` command, with the time of `nomux ls --json` for comparison. The argument is the number of sessions. Each session has three processes. [docs/memory.md](../docs/memory.md) has the results.

```sh
cargo build --release --workspace
cargo run --release -p nomux-end-to-end --example memory_measurements -- 50
```

A run with 50 sessions takes approximately 3 seconds.

`measurements/restore.rs` measures the start of the daemon after a restart of the host. It starts sessions that each fill their journal to the size limit of 32 MiB, stops each holder and the daemon with `SIGKILL`, and measures the time of one `nomux ls --json` command when no daemon runs: the start of the daemon, the read of the state directories and the list. The argument is the number of sessions. [docs/daemon.md](../docs/daemon.md) has the result.

```sh
cargo build --release --workspace
cargo run --release -p nomux-end-to-end --example restore_measurements -- 50
```

A run with 50 sessions takes approximately 1 minute and needs 1.6 GiB in `/tmp`.
