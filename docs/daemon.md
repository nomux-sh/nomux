# Daemon

This document describes the daemon (`nomuxd`): its process model, its socket, its lifecycle, how it finds and holds the holders, how it attaches clients to the terminal of a session, and where it runs the extensions.
The code is in `crates/nomuxd`. The transport is in `crates/nomux-transport`, and the client side is in `crates/nomux-client`.

[architecture.md](architecture.md) shows where the daemon is in the core. [holder.md](holder.md) describes the holder and its contract with the daemon. [cli.md](cli.md) describes the commands that use the daemon. [remote.md](remote.md) describes how a client on a different computer reaches the daemon through SSH. [extensions.md](extensions.md) describes the extension host of the daemon.

## Process model

One daemon runs for each user and each runtime directory. The runtime directory is `~/.nomux/run`.
The daemon is one process with one thread. It uses a `tokio` runtime of the `current_thread` type, because it waits for I/O almost all the time.
The installation and the removal of an extension read, copy and delete files. For this work the runtime starts a second thread, which ends 10 seconds after the work.

```
nomux (CLI) --- Unix socket: envelopes ---> nomuxd --- Unix socket: holder protocol ---> nomux-holder
            <-- Unix socket: frames ------>   |   <--- ring buffer (shared memory) ----  nomux-holder
                                              +--- starts ---> nomux-holder              ...
                                              +--- starts, pipes: frames ---> process of an extension in use
```

The daemon owns no PTY. Each session lives in its holder. Thus the end of the daemon ends no session.

### Start

```sh
nomuxd            # runs in the foreground; the log goes to the standard error
nomuxd --detach   # starts the daemon as a separate process and ends
nomuxd --replace  # if a daemon runs, stops it and takes its place; also with --detach
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
5. Read the installed extensions from the directory of the extension host. Start no extension.
6. Accept connections.

A client that connects during step 4 or step 5 waits in the queue of the socket.

### Log

The daemon writes `tracing` events to its standard error. The default level is `warn`: a daemon without a problem writes nothing.
The `NOMUX_LOG` variable sets the level: `error`, `warn`, `info`, `debug` or `trace`.
The daemon never copies the standard error of an extension to its log. A failure of an extension is one line with the level `warn`.

### Stop

The daemon stops when it gets `SIGTERM` or `SIGINT`, when a client sends `Host.Stop` (`nomux daemon stop`), or when it loses the lease of a holder.
The stop sequence is:

1. Stop accepting connections.
2. Stop the process of each extension that runs. A process has 3 seconds to end after the request, then the daemon ends it. A call of an extension that waits fails.
3. Wait until each request that began has its response.
4. Remove the socket file and release the instance lock.
5. Close the connections and end with status 0.

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
| `Session.Attach` | Connects the client to the terminal of a session. After the response, the connection carries terminal frames: see "Attach" |
| `Session.Detach` | Detaches each client that is attached to a session and gives their number |
| `Session.Input` | Writes bytes to the terminal of a session and answers when the holder has them |
| `Session.Resize` | Changes the window size of a session |
| `Ext.List` | Gives the installed extensions with their state and their approved permissions |
| `Ext.Inspect` | Validates an extension package in a directory and gives the permissions that its manifest asks for. Installs nothing |
| `Ext.Install` | Installs an extension package from a directory with the permissions that the user approved |
| `Ext.Remove` | Stops an extension and removes its package, its state and its data |
| `Ext.SetEnabled` | Enables or disables an extension |
| `Ext.Call` | Calls a method of an extension. The first call starts the process of the extension |

[extensions.md](extensions.md) describes the requests of the `Ext` service, the directory of the extensions and the host services that an extension can call.

No capability flag is active. The daemon sends an empty `host_id`, because it has no host identity yet.

## One daemon for each runtime directory

The daemon holds an exclusive lock (`flock`) on the file `nomuxd.lock` in the runtime directory for all of its life. The file contains the process ID of the daemon.
The operating system releases the lock when the process ends, also after `kill -9`. Thus there is no stale lock.

A second daemon for the same runtime directory cannot take the lock. It writes `a daemon already runs for <directory> (process ID <n>)` to its standard error and ends with status 1.
It does this before it removes the socket file and before it connects to a holder. Thus the first daemon and its leases are not changed.

### Replacement of a daemon

`nomuxd --replace` takes the place of a daemon that runs. A client uses it for a daemon that has no common protocol version with the client: [remote.md](remote.md) says when.

1. The new daemon cannot take the lock. It reads the process ID of the first daemon from the lock file and sends `SIGTERM` to it.
2. The first daemon does its stop sequence: it answers the requests that it began, removes its socket, releases the lock and ends.
3. The new daemon gets the lock. If that does not occur in 15 seconds, it ends with an error. Then it does the normal start sequence: it makes its socket, finds the holders and takes their leases.

No session ends, and the two daemons never serve at the same time. The clients that are attached through the first daemon lose their connection.
A daemon writes its process ID immediately after it takes the lock. Until then the lock file has the process ID of an earlier daemon. Thus the new daemon reads the file two times, 20 ms apart, and sends the signal only if the two values are the same.

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

1. The task connects to the control socket and reads the hello with `recvmsg`, because the descriptor of the ring buffer comes with it. If the session identifier in the hello is not the identifier of the directory, the connection fails.
2. It sends `AcquireLease`. The holder gives the lease to the connection that asks last.
3. It sends `WaitOutput` with the position `u64::MAX`. The ring buffer never gets to this position, thus the holder answers only when the command has ended. The daemon needs no timer to learn the end of a command.
4. When the answer comes, the task sends `Status`. The holder gives the exit status, removes its directory and ends. The daemon removes the session from its list.

This task is the only writer: `Input`, `Resize` and `Signal` go on its connection, because only this connection has the lease. A request handler or an attached client gives the task a command on a queue of 64 entries. A sender waits when the queue is full, thus input that the holder does not take uses no more memory.
The task does not read terminal data. It closes the descriptor of the ring buffer that comes with its hello.

**Two daemons cannot write to one holder.** The instance lock prevents two daemons in one runtime directory. The lease is the second protection: only the connection with the lease can send input, a new window size or a signal.
If a holder answers a write request with `LeaseRequired`, a different daemon took the lease. The daemon then refuses the request of the client, writes an error to its log and stops. It does not take the lease back.

### End of a session

For `Session.Kill`, the task of the session sends `Signal(SIGHUP)` to the command, which is the signal that the end of a terminal gives. If the command still runs after 2 seconds, the task sends `Signal(SIGKILL)`.
The request is complete when the holder reports the exit status. If that does not occur in 10 seconds, the request fails with the code 3 (timeout).
A request for an identifier that no session has fails with the code 2 (not found).

When the command of a session ends, the task gets the exit status from the holder, removes the session from the list and gives the exit status to each attached client. The holder ends when the daemon has the exit status, thus only the clients that are attached at that moment get it.

### Lost connection

If the connection to a holder fails, the session gets the state "unreachable" and the task scans the runtime directory for the state of its holder:

- The holder is `Live`: the task connects again and takes the lease again.
- The holder is `Stale`, or its directory is gone: the task removes the directory and the session leaves the list. The attached clients get the end of the session without an exit status.
- In all other cases, the task waits and tries again. The pause starts at 100 ms and becomes two times longer after each attempt, up to 5 seconds. There is no tight loop.

A request to end the session that arrives during this time waits for the connection. A request for the terminal of an unreachable session (`Session.Attach`, `Session.Input`, `Session.Resize`) fails immediately with the code 1.

## Attach

An attach connects one client to the terminal of one session. The specification is [`spec/frame.md`](https://github.com/nomux-sh/proto/blob/main/spec/frame.md) of the `proto` repository: the frames, the sequence numbers, the resume, the rule for a slow client and the end of an attach. This section describes how the daemon does it.

```
                        +-- nomuxd ---------------------------------------------+
client A <-- frames --- | attachment A: ring reader A <---+                     |
client A --- Input ---> | attachment A ---+               |  ring buffer        | <--- nomux-holder
client B <-- frames --- | attachment B: ring reader B <---+  (shared memory)    |
client B --- Input ---> | attachment B ---+                                     |
                        |                 v                                     |
                        |  commands -> task of the session --- Input, Resize -> | ---> nomux-holder (lease)
                        |  output feed: Redraw, WaitOutput <------------------> | <--> nomux-holder
                        +-------------------------------------------------------+
```

### Start

A client connects, does the handshake and sends `Session.Attach` as a normal request. The daemon does these steps:

1. It finds the session in the session list. This is the only time that an attach uses the list: after that, the attachment has its own references to the session, and terminal bytes go through no shared lock.
2. If the request has a window size, the task of the session sends `Resize` to the holder, and the daemon waits for the answer.
3. It opens the output feed of the session, if no other client has it open, and maps the ring buffer for a reader of its own.
4. It sends `SessionAttachResponse`. From this moment the connection carries terminal frames in the two directions, and the request is complete.
5. It sends the screen and then the output.

### Output feed

The connection of the task of the session waits for the end of the command with a `WaitOutput` request that the holder answers only then, and a connection can have one `WaitOutput` request. Thus the daemon opens a second connection to the holder for the terminal data, without the lease: the output feed.

- The feed exists only while a client is attached to the session. A session without a client costs the daemon nothing when its command writes: the daemon is not told.
- The feed keeps one `WaitOutput` request with the position that the holder reported last. Each answer wakes the attachments that wait for output. An answer without new output means that the command has ended.
- The feed sends `Redraw` for an attachment that needs the screen, one request at a time, and copies the redraw sequence out of the memory that comes with the reply.
- All the attachments of one session share one feed. A holder accepts 4 connections, and a daemon uses 2 of them at most.

### From the ring buffer to the client

Each attachment has its own reader of the ring buffer and one buffer of 64 KiB. It does this in a loop:

1. Read the ring buffer into the buffer.
2. If there are bytes, send them as one `Output` frame and wait until the socket of the client took the frame. The `seq` of the frame is the position of its first byte in the ring buffer.
3. If there are no bytes, wait for the output feed.

The first step of an attach is the screen: the attachment asks the feed for a redraw, moves its reader to the position of the reply, reads the bytes from there, sends the redraw sequence in `ScreenSnapshot` frames and then the bytes. The redraw position rule of [holder.md](holder.md) makes the two parts fit: no byte is absent and no byte comes two times.

**Resume.** If the request has the epoch of the session and a sequence number, the attachment moves its reader to that position. If the ring buffer still has the bytes from there, the client gets them and no snapshot. In all other cases the client gets the screen. Each session has the epoch 1, and the positions come from the holder, thus a resume also works after a restart of the daemon.

**Slow client.** The attachment reads the ring buffer only after the client took the last frame. The daemon holds no other output for a client: the 64 KiB buffer and the send buffer of the socket are the limit, for each rate and each length of output.
The holder does not know the clients and never waits. If a client is slow, the holder writes over the bytes that the reader of that client needs next, and the reader reports an overrun. The attachment then sends the screen again, as at the start, and continues from the new position. The client gets the latest screen and not the bytes between.
A reader belongs to one client. A slow client changes nothing for a different client of the same session.

**Fairness.** One thread serves all the clients. After each `Output` frame, an attachment gives the thread to the other tasks. Thus a flood of output in one session does not keep the input of a different session waiting.

### Input

The client sends `Input` frames on the same connection, in the direction that the output does not use. The attachment reads them in the same task as it sends the output, and it reads first: a client that does not take its output still gets its input to the session.
The bytes of a frame go to the task of the session as one command, and the task sends them to the holder as `Input` requests of 64 KiB at most. The daemon does not wait for the answer of the holder. Input for a command that has ended is discarded.

`Session.Input` uses the same command with an answer: the response comes after the holder has the bytes.

### Window size

A session has one window size: the size of the last `Session.Attach` with a size or of the last `Session.Resize`. This is the rule of the control protocol. The daemon keeps no size for each client and does not tell the other clients a new size.
The holder refuses a window with more than 1048576 cells, and the daemon refuses it before it asks the holder.

### End of an attach

| Event | What the daemon does |
|---|---|
| The command of the session ends | Each attachment sends the output that the ring buffer still has, then `End` with the exit code or the signal, and closes the connection |
| `Session.Detach` | Each attachment of the session sends `End` with `Detached` and closes the connection. A client that did not take a part of a frame gets no `End`: its connection closes |
| The client closes the connection, or its process ends | The attachment ends. Nothing else changes |
| The holder is gone | The task of the session reports the end without an exit status, and each attachment sends `End` with `Lost` |
| The output feed fails and the session does not end in 2 seconds | The attachment closes the connection. The client can attach again |
| The daemon stops | The connections close without an `End` frame |

When the last attachment of a session ends, the output feed closes its connection to the holder.

### Unsafe code and threads

The daemon has no `unsafe` code. A reader of a ring buffer is a mapping of shared memory, which stays on the thread that made it. Thus the tasks of the client connections are local tasks of the one thread of the daemon (`tokio::task::LocalSet`).

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
| Size of `nomuxd` (glibc, dynamic), before the extension host | 878 KiB (899504 bytes) | < 10 MB |
| Size of `nomuxd` (glibc, dynamic), with the extension host | 1541 KiB (1578192 bytes) | < 10 MB |
| Size of `nomuxd` (musl, static) | 974 KiB (996952 bytes) | < 10 MB |
| Size of `nomux` (glibc, dynamic), before the SSH client | 943 KiB (966104 bytes) | no target |
| Size of `nomux` (musl, static), before the SSH client | 1045 KiB (1069952 bytes) | no target |
| Resident memory, idle daemon without a session (glibc) | 2.9 MB | < 10 MB |
| Resident memory, idle daemon with 2 sessions (glibc) | 3.1 MB | < 10 MB |
| Resident memory, idle daemon with 1 session and 1 attached client (glibc) | 3.1 MB | < 10 MB |
| Resident memory, idle daemon with 1 session (musl, static) | 1.1 MB | < 10 MB |
| Resident memory with the extension host, idle daemon without a session and without an extension (glibc) | 3.7 MB to 3.9 MB | < 10 MB |
| The same daemon without the extension host, in the same measurement | 3.3 MB to 3.4 MB | < 10 MB |
| Resident memory with the extension host, idle daemon with 2 sessions (glibc) | 4.0 MB | < 10 MB |
| Resident memory, idle daemon with 1 installed extension that is not in use (glibc) | 4.1 MB | < 10 MB |
| Resident memory of the same daemon after the extension ran and stopped after its idle time | 4.2 MB | < 10 MB |
| CPU time and context switches in 30 s, idle daemon with 1 installed extension that is not in use | 0 and 0 | 0 |
| Threads of the idle daemon with 1 installed extension that is not in use | 1 | no target |
| CPU time and context switches in 30 s, idle daemon with 2 sessions | 0 and 0 | 0 |
| CPU time and context switches in 30 s, idle daemon with 1 attached client | 0 and 0 | 0 |
| Key to echo on the screen of `nomux attach`, local session: median, 99th percentile | 0.2 ms to 0.5 ms, 0.5 ms to 5.4 ms | < 10 ms |
| The same during a flood of 10 MB/s in a second attached session | 0.2 ms to 0.5 ms, 1.4 ms to 4.7 ms | no change that matters |
| The same during a flood without a limit in a second attached session (19 MB/s to 36 MB/s to its client) | 0.2 ms to 0.8 ms, 3.1 ms to 5.3 ms | no change that matters |
| Growth of the resident memory during a flood of 32 MiB to a client that takes no output | less than 8 MiB (the limit of the test) | no growth with the length of the output |

"Key to echo" is the time from the write of one key to the terminal of `nomux attach` until the terminal has the echo of that key. The session runs `cat`, and the terminal of the session echoes the key. The path has the CLI, the daemon, the holder and the terminal of the session, two times. The program `tests/measurements/attach.rs` types 2000 keys with 2 ms between them: [tests/README.md](../tests/README.md) says how to run it. Each range is the result of 3 runs. The largest value of a run is between 3 ms and 58 ms: a small number of keys wait for the scheduler of a computer that does other work.

The rows "with the extension host" are from one measurement of the two builds on the same computer: the daemon before the extension host has 0.4 MB less there, most of it for the larger binary. The earlier rows are from an earlier measurement and an earlier build.

The commands, with `$pid` as the process ID of the daemon from `nomux daemon status --json`:

```sh
ls -l target/release/nomuxd
grep VmRSS /proc/$pid/status
awk '{print $14 + $15}' /proc/$pid/stat     # CPU time in clock ticks: the same value before and after 30 s
grep ctxt /proc/$pid/status                 # the same values before and after 30 s
grep Threads /proc/$pid/status
```

## Open points for the next versions

These parts are not in the daemon yet:

- **Journal.** The daemon has only the output that the ring buffer of the holder has. A client that was away for more output gets the screen, and the scrollback is not sent.
- **Second send mode.** A client that is behind gets the full screen again. The mode that sends only the rows that changed is not written.
- **Window size for each client.** The other clients are not told a new size, and a new session always starts with 24 rows and 80 columns.
- **Input that the terminal does not take.** A holder reads no request while it keeps 64 KiB of input that the terminal of the session did not take. `Session.Kill` then waits until the command reads its input.
- **Parallel requests.** The daemon answers the requests of one connection in their sequence. A long request delays the next request of the same connection, not of other connections.
- **State.** The daemon keeps no database. After a restart it knows the identifier, the state and the process ID of a session, which come from the holder, and not the command or the working directory.
- **Exit status.** A session leaves the list when its command ends. The clients that are attached at that moment get the exit status. The daemon does not keep it for a later request.
- **Session environment.** A session gets the environment of the daemon, thus of the client that started the daemon.
- **Daemon update.** `nomuxd --replace` takes over from a daemon that runs, and the clients of that daemon lose their connection. An update that keeps the connections is not written.
- **Service files.** There is no systemd unit and no launchd file.
- **Log rotation.** `nomuxd.log` has no size limit. With the default level it stays almost empty.
- **Host identity, authentication, warm pool, restore after a reboot, the QUIC transport.**
