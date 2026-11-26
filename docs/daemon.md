# Daemon

This document describes the daemon (`nomuxd`): its process model, its socket, its lifecycle, how it finds and holds the holders, how it attaches clients to the terminal of a session, what it keeps of a session without a process, how it starts such a session again, and where it runs the extensions.
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
4. Read the global configuration file: see "Configuration".
5. Scan the runtime directory for holders. Remove the stale directories and connect to the live holders.
6. Read the session state directories. A session that has no live holder becomes dormant, ended or damaged: see "Sessions without a holder". The daemon starts no process in this step.
7. Read the installed extensions from the directory of the extension host. Start no extension.
8. Accept connections.
9. Start the command of each dormant session that has the restore policy `auto`.

A client that connects during steps 5 to 7 waits in the queue of the socket. Step 9 runs while the daemon answers clients.

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
| `Session.List` | Gives the sessions, their state and their description. The ended sessions are in the response only if the request asks for them |
| `Session.Kill` | Ends a session and answers when its command has ended. Removes a dormant or a damaged session with its state |
| `Session.Attach` | Connects the client to the terminal of a session, and starts a dormant session again. After the response, the connection carries terminal frames: see "Attach" |
| `Session.Start` | Starts the command of a dormant session again: see "Restore" |
| `Session.Capture` | Gives output bytes of a session that are before a position: see "Scrollback" |
| `Session.SetRestorePolicy` | Sets when the daemon starts the command of a session again |
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

The daemon sends the capability flag `output_history` in its `Welcome`. If the `Hello` of a client has the flag too, an attach of that connection can get the output of an earlier epoch and the frames `HistoryGap` and `Restored`: see "Attach". The daemon sends an empty `host_id`, because it has no host identity yet.

### Configuration

The daemon reads the global configuration file one time, at its start: `~/.config/nomux/config.toml`, or the path that [remote.md](remote.md) gives for the other cases. A change of the file applies after the next start of the daemon.

```toml
[restore]
default_policy = "on-focus"   # auto | on-focus | manual

[sessions]
ended_retention = "24h"       # a number with the unit s, m, h or d
```

| Key | Default | Function |
|---|---|---|
| `restore.default_policy` | `on-focus` | The restore policy of a session that has none of its own |
| `sessions.ended_retention` | `24h` | How long the daemon keeps a session after its command ended |

A file that is not valid does not stop the daemon, because the sessions need it. The daemon writes the problem to its log and uses the defaults.

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
2. It starts `nomux-holder --session <identifier> --runtime-directory <directory> --state-directory <directory> --working-directory <directory> -- <command>`. It finds `nomux-holder` in the directory of its own binary, and then in `PATH`. The state directory is the state directory of the user: [holder.md](holder.md) has its path.
3. It waits until the holder writes its `ready` file. If the holder ends before that, the request fails with the last line of the standard error of the holder. If the holder is not ready after 5 seconds, the daemon stops it and the request fails with the code 3 (timeout).
4. It connects to the holder and takes the lease.

If the holder does not start, the daemon removes the session state directory that the holder made. Thus a session that never ran is not a dormant session after the next start of the daemon.

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
4. When the answer comes, the task sends `Status`. The holder gives the exit status, removes its directory and ends. The session is an ended session then: see "Sessions without a holder".

This task is the only writer: `Input`, `Resize` and `Signal` go on its connection, because only this connection has the lease. A request handler or an attached client gives the task a command on a queue of 64 entries. A sender waits when the queue is full, thus input that the holder does not take uses no more memory.
The task does not read terminal data. It closes the descriptor of the ring buffer that comes with its hello.

**Two daemons cannot write to one holder.** The instance lock prevents two daemons in one runtime directory. The lease is the second protection: only the connection with the lease can send input, a new window size or a signal.
If a holder answers a write request with `LeaseRequired`, a different daemon took the lease. The daemon then refuses the request of the client, writes an error to its log and stops. It does not take the lease back.

### End of a session

For `Session.Kill`, the task of the session sends `Signal(SIGHUP)` to the command, which is the signal that the end of a terminal gives. If the command still runs after 2 seconds, the task sends `Signal(SIGKILL)`.
The request is complete when the holder reports the exit status. If that does not occur in 10 seconds, the request fails with the code 3 (timeout).
A request for an identifier that no session has, or for an ended session, fails with the code 2 (not found).
For a dormant or a damaged session, `Session.Kill` has no command to end: the daemon removes the session and its state directory, thus also its output.

When the command of a session ends, the task gets the exit status from the holder and gives it to each attached client. The session stays in the list as an ended session, with its exit status, for the retention time.

### Lost connection

If the connection to a holder fails, the session gets the state "unreachable" and the task scans the runtime directory for the state of its holder:

- The holder is `Live`: the task connects again and takes the lease again.
- The holder is `Stale`, or its directory is gone: the task removes the directory. The attached clients get the end of the session without an exit status. The command did not end, thus the session is dormant now. If a client asked for the end of the session before, the session leaves the list with its state directory.
- In all other cases, the task waits and tries again. The pause starts at 100 ms and becomes two times longer after each attempt, up to 5 seconds. There is no tight loop.

A request to end the session that arrives during this time waits for the connection. A request for the terminal of an unreachable session (`Session.Attach`, `Session.Input`, `Session.Resize`) fails immediately with the code 1.

## Sessions without a holder

The holder of a session keeps the description of the session and its output in the session state directory: the `session` file and the journal. [holder.md](holder.md) has the format. The directory stays when the holder ends, when it is killed and when the host starts again. Thus the daemon knows a session that has no process.

| State | Condition | What the daemon has |
|---|---|---|
| `dormant` | No holder runs, and the `session` file says that the command ran | The description and the output. A restore starts the command again |
| `ended` | The `session` file has an exit status, or the holder gave the exit status to this daemon | The description, the exit status, the time of the end and the output, for the retention time |
| `damaged` | No holder runs, and the `session` file cannot be read | The reason, and the journal if it can be read |

A session whose holder is gone is the same case for each cause: a restart of the host, `SIGKILL` for the holder, or a holder that failed. After a restart of the host, each session is dormant.

- The daemon reads the state directories at its start, and it reads one directory again when it loses the holder of a session.
- `Session.List` gives each session with its description from the `session` file: the command, the working directory, the window size, the start time and the epoch. The file has no environment variable, thus the response has none.
- The daemon starts no process for a dormant session until a restore.
- **Nothing removes the state of a session that runs or of a dormant session, but `Session.Kill`.** The size limit of the journal, which the holder applies, is the only limit of its disk use.

### Restore

A restore starts the command of a dormant session again. The daemon starts a new holder with the session identifier, the command, the working directory and the window size of the `session` file. The new holder writes to the same state directory and takes the next epoch: [holder.md](holder.md) has the rule. The processes of the earlier epoch do not come back, and their output stays in the journal.

Each session has a restore policy:

| Policy | The daemon starts the command again |
|---|---|
| `auto` | At the start of the daemon, and for `Session.Attach` and `Session.Start` |
| `on-focus` | For `Session.Attach` and `Session.Start`. This is the default |
| `manual` | Only for `Session.Start`. `Session.Attach` fails with the code 1 and names the command `nomux start` |

- `Session.SetRestorePolicy` stores the policy in the file `restore-policy` of the session state directory: one line with the name of the policy. A session without the file has the policy of `restore.default_policy`.
- **The daemon owns this file.** The holder owns the `session` file and writes it again from its memory, thus a value that the daemon puts there does not stay. The daemon does not read the `restore` key of the `session` file.
- A `restore-policy` file that does not have the name of a policy counts as absent.
- The policy `auto` applies only at the start of the daemon. A session that becomes dormant while the daemon runs waits for a client.
- The daemon does one restore at a time. Two clients that open the same dormant session start one holder.
- An attach with a window size starts the command with that size.
- If the command cannot start, for example because its working directory is gone, the request fails with the code 1 and the session stays dormant.

The environment of the new command is the environment of the daemon at the time of the restore. The environment of the earlier command is not stored: it can have secrets.

### Ended sessions and the retention time

When the command of a session ends, the daemon keeps the session and its state directory for the retention time (`sessions.ended_retention`, 24 hours by default). Then it removes the two.

- An ended session is in `Session.List` only if the request asks for it (`nomux ls --all`). `Host.Info` does not count it.
- `Session.Capture` reads the output of an ended session. Each other request for it fails with the code 2.
- The retention time starts at the end of the command. A daemon that starts later reads the time of the end from the `session` file, thus a restart of the daemon does not make the time longer. A session whose time is over at the start of the daemon is removed at once.
- **The daemon does not poll.** Each ended session has one timer for the end of its retention time. The timer wakes the daemon one time. A daemon without an ended session has no timer, and an idle daemon with ended sessions uses no CPU time until a timer ends.
- The timer uses the monotonic clock of the host. While the host sleeps, that clock does not run on Linux, thus the removal can be later than the retention time.

### Damaged state files

The daemon trusts no file of a state directory. A file can be cut, changed by a person, or made by a different program.

- A `session` file that is not a regular file, that is longer than 8 MiB, or that the parser of [holder.md](holder.md) refuses gives a `damaged` session with the reason. The daemon examines the type of a file before it opens it: to open a FIFO stops a process.
- At its start, the daemon reads the first 32 bytes of each segment file of a journal. A segment that does not start with the header of its name gives the session a damage note with the name of the file. The session keeps its state: it can start again, and only a part of its output is gone.
- Damage inside a segment is found when the output is read. The reader gives the bytes before the damage and continues with the next segment.
- A directory entry whose name is not a session identifier is ignored.
- A damaged session cannot start again. `Session.Kill` removes it with its state directory.

A damaged file never stops the daemon or a different session.

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
2. If the session is dormant and its restore policy is not `manual`, the daemon starts its command again: see "Restore".
3. If the request has a window size, the task of the session sends `Resize` to the holder, and the daemon waits for the answer. A session that started in step 2 has the size already.
4. It opens the output feed of the session, if no other client has it open, and maps the ring buffer for a reader of its own.
5. It sends `SessionAttachResponse` with the epoch that the holder gave in its hello. From this moment the connection carries terminal frames in the two directions, and the request is complete.
6. It sends the screen and then the output.

### Output feed

The connection of the task of the session waits for the end of the command with a `WaitOutput` request that the holder answers only then, and a connection can have one `WaitOutput` request. Thus the daemon opens a second connection to the holder for the terminal data, without the lease: the output feed.

- The feed exists only while a client is attached to the session or reads its scrollback. A session without a client costs the daemon nothing when its command writes: the daemon is not told.
- The feed keeps one `WaitOutput` request with the position that the holder reported last. Each answer wakes the attachments that wait for output. An answer without new output means that the command has ended.
- The feed sends `Redraw` for an attachment that needs the screen and `JournalRead` for a client that needs output of the journal. The two replies come with shared memory, thus the feed sends one of these requests at a time and copies the bytes out of the memory of the reply.
- All the attachments of one session share one feed. A holder accepts 4 connections, and a daemon uses 2 of them at most.

### From the ring buffer to the client

Each attachment has its own reader of the ring buffer and one buffer of 64 KiB. It does this in a loop:

1. Read the ring buffer into the buffer.
2. If there are bytes, send them as one `Output` frame and wait until the socket of the client took the frame. The `seq` of the frame is the position of its first byte in the ring buffer.
3. If there are no bytes, wait for the output feed.

The first step of an attach is the screen: the attachment asks the feed for a redraw, moves its reader to the position of the reply, reads the bytes from there, sends the redraw sequence in `ScreenSnapshot` frames and then the bytes. The redraw position rule of [holder.md](holder.md) makes the two parts fit: no byte is absent and no byte comes two times.

**Epoch.** The epoch and the positions come from the holder: the epoch is in its hello, and a position is the number of bytes that its command wrote. The daemon invents neither. Thus a resume also works after a restart of the daemon. Each frame has the epoch of its bytes.

**Resume.** If the request has the epoch of the session and a sequence number, the attachment continues at that position:

1. If the ring buffer still has the bytes from there, the client gets them and no snapshot.
2. If not, the attachment asks the holder for the bytes of the journal (`JournalRead`), sends them, and tries the ring buffer again after each reply. When the ring buffer has the next byte, the attachment continues as in each other attach. The client gets one run of `Output` frames without a gap and without a repeat.
3. If the journal does not have the byte, because the size limit removed it or a file is damaged, the bytes are gone. A client with the capability flag `output_history` gets a `HistoryGap` frame, then the oldest state that the journal has after the gap, as a snapshot, and each byte from there on. The state is the last checkpoint of the oldest segment that has one. A client without the flag, and a journal without a checkpoint, get the current screen.

A position that the holder did not write yet is not a position of the epoch: the client gets the screen.

The attachment reads the journal only at its start. While it sends bytes of the journal, it holds one reply of the holder: 262144 bytes at most.
If the command ends during that time, the holder ends too. The attachment then reads the segment files itself, which have each byte after the end of a command, and sends the rest.

**An earlier epoch.** A client that sends an epoch before the current one was attached before a restore. With the capability flag `output_history`, it gets the output of that epoch from its position to the end, from the segment files, then a `Restored` frame, then the screen of the current epoch.
The client whose attach started the command again has no position. It gets the last checkpoint of the earlier epoch as a snapshot, the output after it, the `Restored` frame and the new screen. The checkpoints are 1 MiB of output apart, thus this is 1 MiB of old output at most. `Session.Capture` gives the output before it.

**Slow client.** The attachment reads the ring buffer only after the client took the last frame. The daemon holds no other output for a client: the 64 KiB buffer and the send buffer of the socket are the limit, for each rate and each length of output.
The holder does not know the clients and never waits. If a client is slow, the holder writes over the bytes that the reader of that client needs next, and the reader reports an overrun. The attachment then sends the screen again, as at the start, and continues from the new position. The client gets the latest screen and not the bytes between. It gets no `HistoryGap` frame: the journal has these bytes, and `Session.Capture` gives them.
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
| The holder is gone | The task of the session reports the end without an exit status, and each attachment sends `End` with `Lost`. The session is dormant then |
| The output feed fails and the session does not end in 2 seconds | The attachment closes the connection. The client can attach again |
| The daemon stops | The connections close without an `End` frame |

When the last attachment of a session ends, the output feed closes its connection to the holder.

### Scrollback

`Session.Capture` gives a client output of a session without an attach: the last bytes before a position, 262144 bytes at most in one response. A client reads the scrollback in parts, from the newest bytes to the oldest, and it asks for the epoch before when an epoch has no more output. [`spec/control.md`](https://github.com/nomux-sh/proto/blob/main/spec/control.md) has the rules of the request.

| Session | Source of the bytes |
|---|---|
| Runs, and the request is for its current epoch | The holder (`JournalRead`): the segment files and the bytes that the holder did not write to a file yet |
| Runs, and the request is for an earlier epoch | The segment files |
| Unreachable, dormant, ended or damaged | The segment files. They can lack the output of the last second before the holder stopped |

- The bytes are the output as the command wrote it, with its escape sequences. The daemon does not make lines or a screen from them.
- A response has bytes without a gap. If a part of the journal is gone, the response has the newest bytes before the position that the journal has without a gap, and the next request continues before them.
- The daemon keeps nothing between two requests. It reads the files for each request.

`nomux capture` uses this request: [cli.md](cli.md) has the command.

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
The daemon does not make that call. A systemd user unit for `nomuxd` must set `KillMode=process`. The unit of "Daemon at login" does.

Measured on Fedora with systemd 259: `systemd-run --user -p KillMode=control-group nomuxd`, then `systemctl --user stop`, ends the holder. The same commands with `KillMode=process` keep the holder, and the next daemon finds the session.

## Daemon at login

A daemon that starts at login makes the sessions with the restore policy `auto` start after a restart of the host, and it has the list of the dormant sessions ready for the first client. Without it, the first `nomux` command starts the daemon, and the result is the same from that moment.

`nomux daemon install-service` writes one file, and `nomux daemon uninstall-service` removes it. With `--dry-run`, the commands print the path and the content and change nothing. The commands write only in the configuration directories of the user, they need no `sudo`, and they start no program: the user gives the file to the service manager with the command that the output shows. [cli.md](cli.md) has the commands.

| Host | File | The user then runs |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/systemd/user/nomuxd.service`, or `~/.config/systemd/user/nomuxd.service` | `systemctl --user daemon-reload && systemctl --user enable --now nomuxd.service` |
| macOS | `~/Library/LaunchAgents/sh.nomux.nomuxd.plist` | `launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/sh.nomux.nomuxd.plist` |

The systemd unit, for a `nomuxd` in `/home/user/.local/bin`:

```ini
[Unit]
Description=nomux daemon
Documentation=https://nomux.sh

[Service]
ExecStart="/home/user/.local/bin/nomuxd" --replace
KillMode=process
Restart=on-failure

[Install]
WantedBy=default.target
```

The launchd agent has the label `sh.nomux.nomuxd`, the program arguments `nomuxd --replace`, `RunAtLoad`, `KeepAlive` with `SuccessfulExit` false, `AbandonProcessGroup` and the process type `Background`.

- **`KillMode=process`.** A stop of the unit stops only the daemon. "What survives the end of the daemon" has the reason: the holders are in the control group of the unit.
- **`--replace`.** The daemon of the service takes the place of a daemon that a client started before. No session ends. Without the option, the service fails while such a daemon runs.
- **`Restart=on-failure`, and `KeepAlive` with `SuccessfulExit` false.** A daemon that fails starts again. A daemon that a client stopped (`nomux daemon stop`) has the exit status 0 and stays stopped.
- **The path of the daemon** is the path of the `nomuxd` in the directory of the `nomux` that wrote the file. After the binaries moved, write the file again.
- `uninstall-service` also removes the link `default.target.wants/nomuxd.service` that `systemctl --user enable` made. The daemon that runs continues.

**Limit: the end of the user manager.** systemd stops the user manager of a user after the last logout, and the user manager then stops each process of its units: the daemon and also the holders, because `KillMode=process` applies only to a stop of the unit. `loginctl enable-linger` keeps the user manager, and thus the sessions, after a logout. A daemon that a `nomux` command started in a login session is not in a unit of the user manager. This behaviour is from the documentation of systemd: it was not examined here, because the test ends each process of the user.

What was examined, on Fedora with systemd 259:

- `systemd-analyze verify --user` accepts the unit that the command writes, also for a path of `nomuxd` with a space. It refuses the same unit without the quotes.
- A transient unit with a temporary name and the properties of the unit (`systemd-run --user -p KillMode=process -p Restart=on-failure nomuxd --replace`, with a temporary home directory): after `systemctl --user stop`, the holder of a session runs, and the next daemon finds the session.
- The property list of the launchd agent was read with a property list parser (`plistlib` of Python), also for a path with `&`, `<` and `>`. **It was not loaded on a Mac.**

## Measurements

Measured on Linux x86-64 with the release profile. The targets are in [bench/README.md](../bench/README.md).

| Measurement | Result | Target |
|---|---|---|
| Size of `nomuxd` (glibc, dynamic), before the extension host | 878 KiB (899504 bytes) | < 10 MB |
| Size of `nomuxd` (glibc, dynamic), with the extension host | 1541 KiB (1578192 bytes) | < 10 MB |
| Size of `nomuxd` (glibc, dynamic), with the extension host, the journal and the restore | 1737 KiB (1778864 bytes) | < 10 MB |
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
| Resident memory, idle daemon with 3 dormant sessions and 1 ended session (glibc) | 4.3 MB | < 10 MB |
| CPU time and context switches in 30 s, idle daemon with 3 dormant sessions and 1 ended session | 0 and 0 | 0 |
| Start of the daemon to a complete `nomux ls --json`, 50 dormant sessions with a journal of 32 MiB each: median, slowest of 11 runs | 25.3 ms, 31.4 ms | < 1 s |
| Key to echo on the screen of `nomux attach`, local session: median, 99th percentile | 0.2 ms to 0.5 ms, 0.5 ms to 5.4 ms | < 10 ms |
| The same during a flood of 10 MB/s in a second attached session | 0.2 ms to 0.5 ms, 1.4 ms to 4.7 ms | no change that matters |
| The same during a flood without a limit in a second attached session (19 MB/s to 36 MB/s to its client) | 0.2 ms to 0.8 ms, 3.1 ms to 5.3 ms | no change that matters |
| Growth of the resident memory during a flood of 32 MiB to a client that takes no output | less than 8 MiB (the limit of the test) | no growth with the length of the output |

"Key to echo" is the time from the write of one key to the terminal of `nomux attach` until the terminal has the echo of that key. The session runs `cat`, and the terminal of the session echoes the key. The path has the CLI, the daemon, the holder and the terminal of the session, two times. The program `tests/measurements/attach.rs` types 2000 keys with 2 ms between them: [tests/README.md](../tests/README.md) says how to run it. Each range is the result of 3 runs. The largest value of a run is between 3 ms and 58 ms: a small number of keys wait for the scheduler of a computer that does other work.

The rows "with the extension host" are from one measurement of the two builds on the same computer: the daemon before the extension host has 0.4 MB less there, most of it for the larger binary. The earlier rows are from an earlier measurement and an earlier build.

"Start of the daemon" is the time of one `nomux ls --json` command when no daemon runs: the CLI starts the daemon, the daemon reads the state directories, and the CLI gets the list. The program `tests/measurements/restore.rs` makes the sessions with the real binaries, stops each holder and the daemon with `SIGKILL`, and measures: [tests/README.md](../tests/README.md) says how to run it. The 50 state directories had 1557 MiB of journal files. The row of the musl binary is from an earlier build: the binary was not built again for this measurement.
The files were on a `tmpfs` file system, thus the value has no cost of a disk. After a restart of the host, the daemon reads from the disk for each session the `session` file and the first 32 bytes of each segment file: approximately 16 segments for a full journal. It does not read the output. That read was not measured with an empty page cache: to empty the cache needs the `root` user.

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

- **Journal.** An attach that starts a session again sends 1 MiB of the old output at most, and it does not send the output of the epochs between the epoch of a client and the current one. The size limit of the journal is a constant of the holder. A client cannot ask for the screen of a position: `Session.Capture` gives bytes, and the daemon does not make lines or a screen from them.
- **Restore.** The policy `auto` applies only at the start of the daemon. A restore gives the command the environment of the daemon: the environment profile of a session is not written. There is no request that starts the command of an ended session again, and none that removes an ended session before its retention time is over. The restore of a workspace layout is the work of the clients.
- **Cold start.** The time of the start with an empty page cache was not measured.
- **Second send mode.** A client that is behind gets the full screen again. The mode that sends only the rows that changed is not written.
- **Window size for each client.** The other clients are not told a new size, and a new session always starts with 24 rows and 80 columns.
- **Input that the terminal does not take.** A holder reads no request while it keeps 64 KiB of input that the terminal of the session did not take. `Session.Kill` then waits until the command reads its input.
- **Parallel requests.** The daemon answers the requests of one connection in their sequence. A long request delays the next request of the same connection, not of other connections.
- **State.** The daemon keeps no database. What it knows about a session after a restart comes from the holder and from the session state directory.
- **Session environment.** A session gets the environment of the daemon, thus of the client that started the daemon.
- **Daemon update.** `nomuxd --replace` takes over from a daemon that runs, and the clients of that daemon lose their connection. An update that keeps the connections is not written.
- **Service files.** The launchd agent was not loaded on a Mac. The commands do not give the file to the service manager, and they do not stop a daemon.
- **Log rotation.** `nomuxd.log` has no size limit. With the default level it stays almost empty.
- **Host identity, authentication, warm pool, hibernation of agents, the QUIC transport.**
