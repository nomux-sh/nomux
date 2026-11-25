# Daemon

This document describes the daemon (`nomuxd`): its process model, its socket, its lifecycle, and how it finds and holds the holders.
The code is in `crates/nomuxd`. The transport is in `crates/nomux-transport`, and the client side is in `crates/nomux-client`.

[architecture.md](architecture.md) shows where the daemon is in the core. [holder.md](holder.md) describes the holder and its contract with the daemon. [cli.md](cli.md) describes the commands that use the daemon.

## Process model

One daemon runs for each user and each runtime directory. The runtime directory is `~/.nomux/run`.
The daemon is one process with one thread. It uses a `tokio` runtime of the `current_thread` type, because it waits for I/O almost all the time.

```
nomux (CLI) --- Unix socket: envelopes ---> nomuxd --- Unix socket: holder protocol ---> nomux-holder
                                              |                                          nomux-holder
                                              +--- starts ---> nomux-holder              ...
```

The daemon owns no PTY. Each session lives in its holder. Thus the end of the daemon ends no session.

### Start

```sh
nomuxd            # runs in the foreground; the log goes to the standard error
nomuxd --detach   # starts the daemon as a separate process and ends
nomuxd --version
```

`nomuxd --detach` starts a second `nomuxd` process without an option and ends immediately. Thus the daemon has no parent that waits for it. The standard error of that daemon is the file `nomuxd.log` in the runtime directory.
The CLI and the client library start the daemon with `nomuxd --detach` when no daemon answers. They find `nomuxd` in the directory of their own binary, and then in `PATH`.

At its start, the daemon calls `setsid`. Thus the end of the terminal or of the process group of the starter does not stop it. If the daemon already leads a process group, which is the case in an interactive shell, `setsid` fails and the daemon stays in the foreground.

The start sequence is:

1. Make the runtime directory with mode 0700 if it does not exist. If the directory belongs to a different user, or if other users can open it, the daemon ends with an error.
2. Take the instance lock (see "One daemon for each runtime directory").
3. Remove the socket file of an earlier daemon and make the socket.
4. Scan the runtime directory for holders. Remove the stale directories and connect to the live holders.
5. Accept connections.

A client that connects during step 4 waits in the queue of the socket.

### Log

The daemon writes `tracing` events to its standard error. The default level is `warn`: a daemon without a problem writes nothing.
The `NOMUX_LOG` variable sets the level: `error`, `warn`, `info`, `debug` or `trace`.

### Stop

The daemon stops when it gets `SIGTERM` or `SIGINT`, when a client sends `Host.Stop` (`nomux daemon stop`), or when it loses the lease of a holder.
The stop sequence is:

1. Stop accepting connections.
2. Wait until each request that began has its response.
3. Remove the socket file and release the instance lock.
4. Close the connections and end with status 0.

A client that sees the end of its connection after `Host.Stop` can start a new daemon immediately.
The holders continue. The next daemon finds them.

## Socket

The daemon listens on the Unix stream socket `~/.nomux/run/nomuxd.sock`.

| Protection | Value |
|---|---|
| Runtime directory | Mode 0700. The daemon does not start if the mode permits access for other users |
| Socket | Mode 0600 |
| Peer | The daemon reads the user ID of the peer and closes a connection from a different user |

The protocol on the socket is the control protocol of the [`proto`](https://github.com/nomux-sh/proto) repository. Each message is one `Envelope` with a varint length prefix. An envelope has 1 MiB at most.

1. The first request of a connection is `Hello` with the protocol version of the client, the oldest version that it supports and its capability flags.
2. The daemon answers with `Welcome`. If the two version ranges have no common version, the daemon answers with an `Error` that has the code 5 and closes the connection. The CLI ends with exit code 5 then. A client that gets a `Welcome` with a range that it cannot use closes the connection and reports the same error.
3. After the handshake, the daemon answers the requests of one connection in their sequence.

The daemon implements these requests:

| Request | Function |
|---|---|
| `Host.Info` | Gives the version of the daemon, its process ID and the number of sessions |
| `Host.Stop` | Stops the daemon. No session ends |
| `Session.Create` | Starts a holder for a new session |
| `Session.List` | Gives the sessions and their state |
| `Session.Kill` | Ends a session and answers when its command has ended |

No capability flag is active. The daemon sends an empty `host_id`, because it has no host identity yet.

## One daemon for each runtime directory

The daemon holds an exclusive lock (`flock`) on the file `nomuxd.lock` in the runtime directory for all of its life. The file contains the process ID of the daemon.
The operating system releases the lock when the process ends, also after `kill -9`. Thus there is no stale lock.

A second daemon for the same runtime directory cannot take the lock. It writes `a daemon already runs for <directory> (process ID <n>)` to its standard error and ends with status 1.
It does this before it removes the socket file and before it connects to a holder. Thus the first daemon and its leases are not changed.

If two clients start a daemon at the same time, one daemon gets the lock, the other one ends, and the two clients connect to the daemon that stays.

## How the daemon finds and holds the holders

### Start of a session

For `Session.Create`, the daemon does these steps:

1. It makes a random session identifier of 8 characters from `a` to `z` and `2` to `7`. It makes a new one if a session or a holder directory already has the identifier.
2. It starts `nomux-holder --session <identifier> --runtime-directory <directory> --working-directory <directory> -- <command>`. It finds `nomux-holder` in the directory of its own binary, and then in `PATH`.
3. It waits until the holder writes its `ready` file. If the holder ends before that, the request fails with the last line of the standard error of the holder. If the holder is not ready after 5 seconds, the daemon stops it and the request fails with the code 3 (timeout).
4. It connects to the holder and takes the lease.

An empty command is the shell of the user: the `SHELL` variable of the daemon, or `/bin/sh`. Without a working directory, the command starts in the home directory. The CLI always sends a working directory.
The command gets the environment of the daemon, and the terminal has 24 rows and 80 columns. There is no request for the environment or for the window size yet.

### Holders of an earlier daemon

At its start, the daemon calls `nomux_holder_protocol::discovery::scan` for the runtime directory.

| State of the holder directory | What the daemon does |
|---|---|
| `Stale` | Removes the directory |
| `Live` | Connects to the holder and takes the lease |
| `Starting` | Puts the session into its list as "unreachable" and connects when the holder is ready |

### Connection and lease

One task owns the connection to one holder for the life of the session.

1. The task connects to the control socket and reads the hello. If the session identifier in the hello is not the identifier of the directory, the connection fails.
2. It sends `AcquireLease`. The holder gives the lease to the connection that asks last.
3. It sends `WaitOutput` with the position `u64::MAX`. The ring buffer never gets to this position, thus the holder answers only when the command has ended. The daemon needs no timer to learn the end of a command.
4. When the answer comes, the task sends `Status`. The holder gives the exit status, removes its directory and ends. The daemon removes the session from its list.

The daemon does not read terminal data yet. It reads the hello as plain bytes, thus the operating system closes the descriptor of the ring buffer that comes with it.

**Two daemons cannot write to one holder.** The instance lock prevents two daemons in one runtime directory. The lease is the second protection: only the connection with the lease can send input, a new window size or a signal.
If a holder answers a write request with `LeaseRequired`, a different daemon took the lease. The daemon then refuses the request of the client, writes an error to its log and stops. It does not take the lease back.

### End of a session

For `Session.Kill`, the task of the session sends `Signal(SIGHUP)` to the command, which is the signal that the end of a terminal gives. If the command still runs after 2 seconds, the task sends `Signal(SIGKILL)`.
The request is complete when the holder reports the exit status. If that does not occur in 10 seconds, the request fails with the code 3 (timeout).
A request for an identifier that no session has fails with the code 2 (not found).

### Lost connection

If the connection to a holder fails, the session gets the state "unreachable" and the task scans the runtime directory for the state of its holder:

- The holder is `Live`: the task connects again and takes the lease again.
- The holder is `Stale`, or its directory is gone: the task removes the directory and the session leaves the list.
- In all other cases, the task waits and tries again. The pause starts at 100 ms and becomes two times longer after each attempt, up to 5 seconds. There is no tight loop.

A request to end the session that arrives during this time waits for the connection.

## What survives the end of the daemon

| Event | Sessions |
|---|---|
| `nomux daemon stop`, `SIGTERM`, `SIGINT` | Continue |
| `kill -9` of the daemon, a daemon failure | Continue |
| `SIGKILL` for the process group of the daemon | Continue |
| The end of the terminal that started the daemon | Continue |
| A stop of a systemd unit with the default `KillMode=control-group` | **End** |
| A stop of a systemd unit with `KillMode=process` | Continue |

These properties make a holder independent of the daemon:

- The holder calls `setsid` at its start. It has its own session and its own process group, thus a signal for the group of the daemon does not reach it.
- The daemon gives the holder no pipe. The standard input and the standard output of the holder are `/dev/null`. The standard error is a file without a name that the daemon reads only if the holder ends before it is ready. A holder that writes after the end of the daemon does not fail.
- The daemon does not set a signal for the end of the parent (`PR_SET_PDEATHSIG`).
- The daemon waits for the exit status of each holder that it started. Thus an ended holder does not stay as a zombie.

The end-to-end tests in `tests/session_lifecycle.rs` kill the daemon and its process group with `SIGKILL` and make sure that the session continues.

**Limit: control groups.** A holder is in the control group of the process that started it, and `setsid` does not change that.
If systemd stops a unit with `KillMode=control-group`, which is the default, it stops each process in the control group of the unit, thus also the holders. A process cannot leave its control group without a call to the service manager (a transient scope through D-Bus).
The daemon does not make that call. A systemd user unit for `nomuxd` must set `KillMode=process`. This repository has no unit file yet.

Measured on Fedora with systemd 259: `systemd-run --user -p KillMode=control-group nomuxd`, then `systemctl --user stop`, ends the holder. The same commands with `KillMode=process` keep the holder, and the next daemon finds the session.

## Measurements

Measured on Linux x86-64 with the release profile. The targets are in [bench/README.md](../bench/README.md).

| Measurement | Result | Target |
|---|---|---|
| Size of `nomuxd` (glibc, dynamic) | 772 KiB (790272 bytes) | < 10 MB |
| Size of `nomuxd` (musl, static) | 866 KiB (886264 bytes) | < 10 MB |
| Size of `nomux` (glibc, dynamic) | 819 KiB (838936 bytes) | no target |
| Resident memory, idle daemon without a session (glibc) | 2.8 MB | < 10 MB |
| Resident memory, idle daemon with 2 sessions (glibc) | 2.9 MB | < 10 MB |
| Resident memory, idle daemon with 1 session (musl, static) | 1.0 MB | < 10 MB |
| CPU time and context switches in 30 s, idle daemon with 2 sessions | 0 and 0 | 0 |

The commands, with `$pid` as the process ID of the daemon from `nomux daemon status --json`:

```sh
ls -l target/release/nomuxd
grep VmRSS /proc/$pid/status
awk '{print $14 + $15}' /proc/$pid/stat     # CPU time in clock ticks: the same value before and after 30 s
grep ctxt /proc/$pid/status                 # the same values before and after 30 s
```

## Open points for the next versions

These parts are not in the daemon yet:

- **Terminal data.** Attach, input and resize. The daemon does not map the ring buffer.
- **Parallel requests.** The daemon answers the requests of one connection in their sequence. A long request delays the next request of the same connection, not of other connections.
- **State.** The daemon keeps no database. After a restart it knows the identifier, the state and the process ID of a session, which come from the holder, and not the command or the working directory.
- **Exit status.** A session leaves the list when its command ends. The daemon does not keep the exit status.
- **Session environment.** A session gets the environment of the daemon, thus of the client that started the daemon.
- **Daemon update.** A new daemon cannot take over from a daemon that runs: the instance lock refuses it.
- **Service files.** There is no systemd unit and no launchd file.
- **Log rotation.** `nomuxd.log` has no size limit. With the default level it stays almost empty.
- **Host identity, authentication, warm pool, restore after a reboot, remote transports.**
