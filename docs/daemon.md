# Daemon

This document describes the daemon (`nomuxd`): its process model, its socket, its lifecycle, how it finds and holds the holders, how it attaches clients to the terminal of a session, what it keeps of a session without a process, how it starts such a session again, how it keeps the status of each session and its events, how it hibernates an idle agent session, how it keeps the workspace tree, and where it runs the extensions.
The code is in `crates/nomuxd`. The transport is in `crates/nomux-transport`, and the client side is in `crates/nomux-client`.

[architecture.md](architecture.md) shows where the daemon is in the core. [holder.md](holder.md) describes the holder and its contract with the daemon. [cli.md](cli.md) describes the commands that use the daemon. [remote.md](remote.md) describes how a client on a different computer reaches the daemon through SSH. [extensions.md](extensions.md) describes the extension host of the daemon.

## Process model

One daemon runs for each user and each runtime directory. The runtime directory is `~/.nomux/run`.
The daemon is one process with one thread. It uses a `tokio` runtime of the `current_thread` type, because it waits for I/O almost all the time.
The installation and the removal of an extension read, copy and delete files, and a request for the memory of the sessions reads the process table of the host. For this work the runtime starts a second thread, which ends 10 seconds after the work.

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
5. Scan the runtime directory for holders. Remove the stale directories and connect to the live holders. Remove the directories of the pooled holders of an earlier daemon that ended, and the directory `pool` with the start files and the named pipes of an earlier daemon: see "Holder pool".
6. Read the session state directories. A session that has no live holder becomes dormant, ended or damaged: see "Sessions without a holder". The daemon starts no process in this step.
7. Read the installed extensions from the directory of the extension host. Start no extension.
8. Accept connections.
9. Start the command of each dormant session that has the restore policy `auto`, but not of a hibernated session.
10. Start the task of the hibernation: see "Hibernation".

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

### Length of the socket paths

The path of a Unix socket has 103 bytes at most on macOS and 107 bytes at most on Linux: `sun_path` has 104 and 108 bytes with the closing zero. The longest socket path in the runtime directory is the control socket of a pooled holder, `<runtime directory>/<session identifier>.pool/sock`: 19 bytes after the runtime directory. Thus a home directory of more than 73 bytes makes this path too long for macOS.

- **The rule.** If the longest socket path of `~/.nomux/run` has more than 103 bytes, nomux opens the runtime directory through the link `/tmp/nomux-<hash>`: `<hash>` is the FNV-1a hash of 64 bits of the path of the runtime directory, in 16 hexadecimal digits. The rule uses the limit of macOS on each host, thus a client knows the socket of a remote host from its home directory only. `nomux_core::runtime_directory::socket_directory` has the rule, and each component uses it: the daemon, the holders that it starts, the CLI and the client of a remote host.
- **The files stay in `~/.nomux/run`.** The link gives them a short path only. The daemon gives the holders the path of the link, thus `NOMUX_SOCK` of a session has the short path.
- **The link is checked.** `/tmp` is open to each user. The daemon and the CLI make the link if it does not exist. A file at its path that is not a link of this user to the runtime directory is an error with the path, and nomux does not remove it. A program that removed the link, for example a cleanup of `/tmp`, does not stop the daemon: the next client or daemon start makes the link again.
- **A path that does not fit.** A holder checks the path of its control socket before `bind`. A path that is too long, for example from a long `--runtime-directory` or a long session identifier, fails with `the socket path <path> has <n> bytes, and the limit on this system is <limit> bytes`.

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
| `Session.Memory` | Measures the memory that the processes of each session use, with the memory limit that the operating system applies. The daemon measures for this request and for a trigger of the hibernation only: [memory.md](memory.md) |
| `Session.Hibernate` | Stops the agent of an agent session that can resume and keeps the session hibernated: [memory.md](memory.md#hibernation) |
| `Session.SetPinned` | Excludes a session from automatic hibernation, or takes the exclusion back |
| `Ext.List` | Gives the installed extensions with their state and their approved permissions |
| `Ext.Inspect` | Validates an extension package in a directory and gives the permissions that its manifest asks for. Installs nothing |
| `Ext.Install` | Installs an extension package from a directory with the permissions that the user approved |
| `Ext.Remove` | Stops an extension and removes its package, its state and its data |
| `Ext.SetEnabled` | Enables or disables an extension |
| `Ext.Call` | Calls a method of an extension. The first call starts the process of the extension |

[extensions.md](extensions.md) describes the requests of the `Ext` service, the directory of the extensions and the host services that an extension can call.

The daemon sends the capability flag `output_history` in its `Welcome`. If the `Hello` of a client has the flag too, an attach of that connection can get the output of an earlier epoch and the frames `HistoryGap` and `Restored`: see "Attach". The daemon sends an empty `host_id`, because it has no host identity yet.

### Configuration

The daemon reads the global configuration file one time, at its start. A change of the file applies after the next start of the daemon. The file has this path:

| Host | Path |
|---|---|
| Each host, if `XDG_CONFIG_HOME` is an absolute path | `$XDG_CONFIG_HOME/nomux/config.toml` |
| Linux | `~/.config/nomux/config.toml` |
| macOS | `~/Library/Application Support/nomux/config.toml` |

The daemon reads no other path. Thus on macOS, a file in `~/.config/nomux` has no effect if `XDG_CONFIG_HOME` is not set. `nomux daemon status` shows the path on the local host. The CLI reads the same file for the hosts of [remote.md](remote.md), and `nomux-core` has the one function that gives the path.

```toml
[restore]
default_policy = "on-focus"   # auto | on-focus | manual

[sessions]
ended_retention = "24h"       # a number with the unit s, m, h or d

[pool]
size = 2                      # 0 to 8; 0 turns the pool off
idle_time = "10m"             # a number with the unit s, m, h or d
shell_start_files = true      # false: the pool takes only sessions in the home directory
```

The memory limits (`memory_limit` and `total_memory_limit` in `[sessions]`) and the settings of the hibernation (the `[agents]` table) are in [memory.md](memory.md#settings).

| Key | Default | Function |
|---|---|---|
| `restore.default_policy` | `on-focus` | The restore policy of a session that has none of its own |
| `sessions.ended_retention` | `24h` | How long the daemon keeps a session after its command ended |
| `pool.size` | `2` | The number of holders that the daemon keeps ready for a new session: see "Holder pool" |
| `pool.idle_time` | `10m` | How long the pool stays when no session starts |
| `pool.shell_start_files` | `true` | A pooled `bash`, `zsh` or `fish` starts with a start file of nomux, which changes to the directory of the session before the first prompt. With `false`, the pool takes only sessions in the home directory: see "Holder pool" |
| `sessions.memory_limit`, `sessions.total_memory_limit` | none | The memory limit of one session and of all sessions: [memory.md](memory.md#memory-limit). With `memory_limit`, the pool is off |
| `agents.*` | see [memory.md](memory.md#settings) | When the daemon hibernates an idle agent session |

The daemon does not read the `[attach]` table: the CLI reads it for the prefix keys of an attach on the computer of the CLI ([cli.md](cli.md#attach)).

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

For `Session.Create` of a session with the shell of the user, the daemon first takes a holder of the pool: see "Holder pool". If the pool has no holder that can take the session, and for a session with a different command, the daemon does these steps:

1. It makes a random session identifier of 8 characters from `a` to `z` and `2` to `7`. It makes a new one if a session, a holder directory or a pooled holder already has the identifier.
2. It starts `nomux-holder --session <identifier> --runtime-directory <directory> --state-directory <directory> --working-directory <directory> -- <command>`. It finds `nomux-holder` in the directory of its own binary, and then in `PATH`. The state directory is the state directory of the user: [holder.md](holder.md) has its path.
3. It waits until the holder writes its `ready` file. If the holder ends before that, the request fails with the last line of the standard error of the holder, or with the message and the location of its panic. If a signal ended the holder, the error also names the signal, says when the standard error is empty, and goes to the log with the level `warn`. If the holder is not ready after 5 seconds, the daemon stops it and the request fails with the code 3 (timeout).
4. It connects to the holder and takes the lease.

A holder that this daemon started and that ends later by a signal or with an exit code that is not 0 gives one line with the level `warn` in the log, with the exit status. After a panic, the file `holder-panic` of the session state directory has the message and the location: [holder.md](holder.md#session-state-directory).

If the holder does not start, the daemon removes the session state directory that the holder made. Thus a session that never ran is not a dormant session after the next start of the daemon.

An empty command is the shell of the user: the `SHELL` variable of the daemon, or `/bin/sh`. Without a working directory, the command starts in the home directory. The CLI always sends a working directory.
The command gets the environment of "Environment of a session", and the terminal has 24 rows and 80 columns. There is no request for the window size yet.

### Environment of a session

A service manager gives the daemon a small environment: launchd gives a job `PATH=/usr/bin:/bin:/usr/sbin:/sbin` and no `TERM` and no `LANG`, and a systemd user service has no `TERM`. Thus the environment of a session cannot be only the environment of the daemon. The rule has one owner: the holder makes the environment of its command (`crates/nomux-holder/src/session_environment.rs`).

| Source | Variables |
|---|---|
| The daemon | Each variable of the environment of the daemon, except the variables of the next rows. The holder gets the environment of the daemon |
| nomux, always | `TERM=xterm-256color`, `COLORTERM=truecolor`, and `NOMUX_SESSION_ID`, `NOMUX_SOCK` and `NOMUX_TOKEN` ([holder.md](holder.md#environment-of-the-command)). A value of the daemon is replaced |
| nomux, if the daemon has no locale | `LC_CTYPE=C.UTF-8` on Linux, `LC_CTYPE=UTF-8` on macOS: a character set of UTF-8. Only if `LC_ALL`, `LC_CTYPE` and `LANG` have no value |
| Never passed | `COLUMNS`, `LINES`, `TERM_PROGRAM` and `TERM_PROGRAM_VERSION`: they describe the terminal of the program that started the daemon |
| The client | Nothing in this version. `Session.Create` has no field for the environment |

- **`TERM` is not the terminal of a client.** The terminal of a session is the terminal of nomux, which follows xterm ([`nomux-term`](../crates/nomux-term/README.md)), and a session has clients of different terminals at different times. tmux does the same with its option `default-terminal`, and sets `COLORTERM=truecolor` too (`environ_for_session` in `environ.c`). zellij sets `TERM=xterm-256color` and `COLORTERM=truecolor` when it starts its server.
- **The locale.** A daemon that a client started has the locale of the terminal of that client. A daemon of a service manager can have none: then the C library uses the locale `C`, which has no characters outside ASCII. The fallback gives a character set of UTF-8 and changes no language, no format of dates and no format of numbers. To give the sessions a language, give `LANG` to the service: `EnvironmentVariables` in the launchd agent, `Environment=` in the systemd unit.
- **Nothing from the client.** tmux copies the variables of its option `update-environment` from the client (`DISPLAY`, `SSH_AUTH_SOCK` and more) at `new-session`. The control protocol has no field for this. Thus a client with a different locale, or with variables such as `SSH_AUTH_SOCK` or `DISPLAY`, does not give them to its session. Issue [#75](https://github.com/nomux-sh/nomux/issues/75) has this gap.
- **The holder pool.** A pooled shell gets the same environment, because no part comes from the client: each pooled shell has the environment of a new holder of the same daemon, and a session never needs a new holder for its environment. A new daemon takes no pooled holder of an earlier daemon, which can have a different environment: see "Holder pool".
- **A restore** gets the environment of the daemon at the time of the restore, with the same rule.

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

## Holder pool

A new session must open in less than 10 ms ([bench/README.md](../bench/README.md)). The start of a holder takes approximately 1 ms, and the start of a shell with its start files takes from 2 ms to more than 200 ms. Thus the daemon keeps a small number of holders ready, each with the shell of the user already started in the home directory: the pooled holders. A new session with the shell takes one. [holder.md](holder.md#pooled-holder) describes a pooled holder.

```
first request ──> pool starts ──> fill: start one pooled holder at a time, up to pool.size
                                    ^                      |
                                    |                      v
              Session.Create ──> take the oldest ──> AssignSession: the holder gives the
                  (restarts the idle time)           shell the directory of the session
pool.idle_time without a new session ──> the pool empties ──> the next Session.Create starts it again
```

**Rules.**

- **Nothing runs before a client.** No pooled holder exists before the first request of a client after the start of the daemon. A daemon that starts at login and gets no request runs no holder. `Host.Stop` does not start the pool.
- **Idle time.** When no session starts for `pool.idle_time` (10 minutes by default), the pool empties: its holders and their shells end. After that, only `Session.Create` starts the pool again. One timer for the end of the idle time wakes the task of the pool: the daemon does not poll.
- **Fill.** The pool starts one pooled holder at a time until it has `pool.size` holders. A take starts the fill again after the response to the client, thus the start of a holder does not delay the new session. If a start fails, the fill stops until the next new session: there is no loop of starts.
- **Pooled holders are not sessions.** They are not in `Session.List`, `Host.Info` does not count them, and they have no state directory and no journal.
- **The end of the daemon ends them.** The standard input of a pooled holder is a pipe from the daemon. When the daemon ends, also after `kill -9`, the pipe closes and each pooled holder ends with its shell. A holder that has its session continues, as each session holder does.
- **A new daemon takes no pooled holder of an earlier daemon.** The shell of such a holder has the environment of the earlier daemon, and a session gets the environment of the daemon that starts it (see "Restore"). Such a holder ends by itself when the pipe of its daemon closes. The new daemon removes the directories of the pooled holders that ended and the directory `pool` of the runtime directory, and starts its own pool after its first request.

**The directory of the session.** A pooled shell started in the home directory, and a session can have a different directory. For `bash`, `zsh` and `fish`, the daemon starts the shell with a start file of nomux. The shell runs the start files of the user while it waits in the pool, and then the start file of nomux reads the directory of the session from a named pipe of the holder, before the first prompt. The read waits until a session takes the holder. Thus the shell shows its first prompt in the directory of the session, nothing is typed into the terminal, and the screen and the journal are the same as for a session with a new holder.

- **The files.** The start files of nomux are in the directory `pool` of the runtime directory, which only the user can open. Their content is fixed: they have no path and no data of the user. The daemon writes them before it starts a pooled holder. The daemon makes the named pipe of a holder, `pool/<session identifier>.directory`, with mode 0600, and the holder removes it: the path is in the variable `NOMUX_DIRECTORY_FIFO`, and the shell reads the directory as data, with a zero byte at its end. Thus each path works, also a path with a quote, a `$`, a newline or a `-` at the start.
- **bash.** `bash --rcfile pool/bashrc`. The file reads `~/.bashrc`, as bash does without `--rcfile`, and then changes to the directory. bash reads the system start file before it, as without `--rcfile`.
- **zsh.** `ZDOTDIR` is `pool/zsh`, and `NOMUX_ZDOTDIR` has the `ZDOTDIR` of the daemon if it has one. `pool/zsh/.zshenv` gives `ZDOTDIR` its value back, reads the `.zshenv` of the user, and puts a `precmd` function first in `precmd_functions`. zsh then reads the other start files of the user from the `ZDOTDIR` of the user. Before the first prompt, the function changes to the directory with `cd -q`, which runs no `chpwd` function, removes itself from `precmd_functions` and removes its definition.
- **fish.** `fish --init-command`, which fish runs after its configuration files.
- **Nothing stays.** Before the first prompt, the shell has no variable, no function and no descriptor of nomux: `NOMUX_DIRECTORY_FIFO`, `NOMUX_ZDOTDIR` and the helper variables are removed, and the redirection of the read closes the named pipe. `OLDPWD` has the value that a shell with a new holder has. The holder removes the named pipe after the shell read it, and at its end.
- **A pipe without a directory.** If the named pipe is gone, or closes without a directory, the shell continues in the home directory and shows its prompt.
- **Restore.** The session metadata has the shell without the arguments of nomux. A dormant session starts again as a session with a new holder.

**The take.** For `Session.Create` without a command, the daemon takes the oldest pooled holder, because its shell had the most time to start, and does these steps:

1. It sends `AssignSession` with the window size of the holder and the working directory of the session. The holder writes the state directory, starts the journal, and then writes the directory to the named pipe of the shell.
2. It connects the session as a session with a new holder: the task of the session, the status, the attach and the restore are the same.

If a step fails, the daemon drops the holder, which ends, removes the state directory that the holder wrote, and takes the next one. With no pooled holder left, it starts a new holder.

**Limits.**

- **The start files run in the home directory.** The shell runs the start files of the user before a session takes it. Thus a start file that reads the working directory reads the home directory. A session in the home directory, and a shell from a new holder, do not have this difference.
- **Other shells.** A shell that is not `bash`, `zsh` or `fish` starts without a start file of nomux. Then the pool takes only a session in the home directory, and a session in a different directory gets a new holder. The same applies when `pool.shell_start_files` is `false`.
- **zsh.** A start file of the user that gives `precmd_functions` a new value, and does not add to it, removes the function of nomux: the shell then stays in the home directory. A system start file that sets `ZDOTDIR` has the same result, because zsh then does not read `pool/zsh/.zshenv`.
- **fish.** fish changes the directory with its `cd` builtin: a function of the user for the event `--on-variable PWD` runs for the change.
- **A command gets a new holder.** A pooled holder can only give its shell. A holder that waits without a command and starts the command at the assignment saves only the start of the holder (approximately 1 ms of the 2.3 ms of a new holder in the daemon) and not the start of the command, which is the long part for a shell. Thus a session with a command always gets a new holder.
- **The size of the pool.** The default of 2 gives a ready holder to two sessions that start one after the other, for example a split of a window, and the second take starts the fill while the first session opens. A pooled holder uses approximately 1 MB with the static musl build, and its shell uses its own memory (4.4 MB for bash 5.3 without start files): a larger default uses memory that most hosts do not need.
- **A shell that starts slowly.** The pool has a holder when the holder is ready, not when its shell completed its start files. A take in that time gives a session whose shell shows its prompt later: the shell completes its start files and then reads the directory.

## Sessions without a holder

The holder of a session keeps the description of the session and its output in the session state directory: the `session` file and the journal. [holder.md](holder.md) has the format. The directory stays when the holder ends, when it is killed and when the host starts again. Thus the daemon knows a session that has no process.

| State | Condition | What the daemon has |
|---|---|---|
| `dormant` | No holder runs, and the `session` file says that the command ran, or the file `hibernation` says that a hibernation ended the command | The description and the output. A restore starts the command again; for a hibernated session, a wake starts the resume command |
| `ended` | The `session` file has an exit status, or the holder gave the exit status to this daemon | The description, the exit status, the time of the end and the output, for the retention time |
| `damaged` | No holder runs, and the `session` file cannot be read | The reason, and the journal if it can be read |

A session whose holder is gone is the same case for each cause: a restart of the host, `SIGKILL` for the holder, or a holder that failed. After a restart of the host, each session is dormant.

- The daemon reads the state directories at its start, and it reads one directory again when it loses the holder of a session.
- `Session.List` gives each session with its description from the `session` file: the command, the working directory, the window size, the start time and the epoch. The file has no environment variable, thus the response has none.
- The daemon starts no process for a dormant session until a restore.
- **Nothing removes the state of a session that runs or of a dormant session, but `Session.Kill`.** The size limit of the journal, which the holder applies, is the only limit of its disk use.

### Names

A session can have a name: `spec/control.md` of the protocol, "Names", has the rule. `Session.Create` with a name and `Session.Rename` store it in the file `name` of the session state directory: one line with the name. A session without the file has no name.

- **The daemon owns this file**, as it owns `restore-policy`. Thus the name stays with a dormant session, after a restart of the daemon and of the host, and with an ended session for the retention time. A file that cannot be read or has no valid name counts as absent: the session has no name and is not damaged.
- One lock covers each check and each change of a name: the check that no other session that is not ended has the name or the identifier, the start of the session and the write of the file. Thus of two `Session.Create` with the same name and `reuse_existing` at the same time, one starts the session and the other gets it. A session without a name does not take the lock.
- A new identifier is not the name of a session that is not ended.
- The daemon gives the name in `Session.List`. It keeps the time of the last attach of each session (`Session.last_attached_at`) in its memory only, and counts the attached clients (`Session.attached_clients`).

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

The environment of the new command is the environment of "Environment of a session", from the daemon at the time of the restore. The environment of the earlier command is not stored: it can have secrets.

A restore of a hibernated session is a wake: the command is the resume command of its file `hibernation`, as words without a shell, and a wake that fails stays hibernated with the reason. [memory.md](memory.md#wake) has the steps.

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
- All the attachments of one session share one feed. A holder accepts 4 connections, and a daemon uses 3 of them at most: the task of the session, the output feed and the status task.

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

## Status of the sessions

The daemon knows what runs in each session: [status.md](status.md) has the model, the sources and the rules, and the `nomux-status` crate has the rules as code. The daemon collects what the sources report.

- **One status task for each session with a holder.** The task opens its own connection to the holder, without the lease. It asks for the token of the session with `SessionToken` (holder protocol 1.4), reads the program reports with `ProgramReports`, and then keeps one `WaitProgramReports` request with the holder. The holder answers it only when a program report changes. Thus the daemon has no timer and does not poll: a session whose programs report nothing costs no CPU time.
- If the connection fails, the task connects again with the pause of "Lost connection". When the session ends, the task stops. A session that is dormant or ended after the end keeps what [status.md](status.md#sessions-without-a-process) says. The removal of an ended session removes its kept run.
- **The status registry.** The status of each session is in a registry of its own, outside the session registry and its lock. An entry has the last report of each source and the Agent run that the resolver made from them. Each change of a source resolves the Agent run again. If the Agent run changed, the registry writes an `AgentRunChanged` event.
- **The foreground program** is read when the daemon resolves: when a different source changes, and for `Agent.Tree` and `Agent.Explain`. The daemon reads the foreground process group of the terminal of the command (field 8 of `/proc/<pid>/stat` on Linux, `e_tpgid` of `proc_pidinfo` on macOS) and the name of the program of that group.
- **Seen.** An attach marks each result of its session as seen, at the start of the attach and at its end.
- **Self-report.** `Agent.SetStatus` must have the token of the current holder of the session. The daemon refuses a wrong token, a session without a process and a holder without a token with the code 4. It compares the token in a time that does not depend on its content, and it writes the token to no log.
- **Limits.** Only a session of the session list has an entry, and 64 ended sessions keep a run at most. The holder keeps 64 program status records at most, the resolver keeps 64 Child work items at most with 8 levels, and a message has 512 bytes at most. Thus a program cannot make the registry grow without a limit.
- A dormant session after a start of the daemon has no entry until a client asks: it is `idle` with no source.

### Events

The daemon keeps the last events in memory: 1024 events at most, and 16 MiB of encoded events at most. An event has the Agent run of a session, which a program of that session makes large, thus the number alone does not limit the memory. The log starts empty at each start of the daemon.

- `Events.Subscribe` makes the connection a stream of events, as `Session.Attach` makes it a stream of frames. The subscription reads the log from its own position and waits for the next event. The log keeps no queue for one client.
- A client that is too far behind, or that asks for a position that the log does not have, gets a snapshot: an `EventsSnapshot` event and the Agent run of each session that is not ended. Then the events after the snapshot follow.
- A subscription ends when the client closes its connection.
- The events of the extensions are in the same log: [extensions.md](extensions.md#events-for-the-clients).

### Hook events

`Hooks.Event` checks the token of the session as `Agent.SetStatus` does, and puts the event into a queue for the adapter extension of the tool: 256 events and 4 MiB at most. The response comes at once. One task calls the method `hook` of the adapters, one event at a time, and ends when the queue is empty. The adapter reports with the host services `agent.report` and `agent.child`, and the status registry keeps the reports as the source "adapter". [adapters.md](adapters.md) has the path of an event and the rules. The code is in `crates/nomuxd/src/hooks`, `crates/nomuxd/src/handlers/hooks.rs` and `crates/nomuxd/src/extensions/agent_services.rs`.

## Hibernation

One task applies the triggers of the hibernation: the count of the live agent sessions, the memory thresholds and the memory pressure of the host. It waits for an event of the event log, a detach, a pressure report of the kernel, and one timer at the time when a session becomes idle long enough if a trigger can then apply. It does not poll. The daemon owns two more files in the state directory of a session: `hibernation` (the last hibernation, as JSON) and `pinned` (empty). [memory.md](memory.md#hibernation) has the triggers, the conditions, the stop, the wake and the measurements. The code is in `crates/nomuxd/src/hibernation`, `crates/nomuxd/src/sessions/hibernate.rs` and `crates/nomuxd/src/memory_limit.rs`.

## Workspace tree

The daemon owns the workspace tree of the host: the Groups and the Projects. It serves the Workspace service (`Workspace.Tree` and the six requests that change the tree) and sends a `WorkspaceChanged` event for each change, in the event log of "Events". It stores the tree with its revision in `workspace.json` in its state directory, with a write that replaces the file in one step, and reads the file only at its start. A file that it cannot use goes aside with a warning in the log, and the daemon starts with an empty tree. The daemon also resolves the symbolic links of the working directory of each session one time, and sends the result in `Session.resolved_working_directory`: a client finds the Project of a session with it. [workspace.md](workspace.md) has the model, the file, the rules and the Project of a session. The code is in `crates/nomuxd/src/workspace.rs`, `crates/nomuxd/src/workspace` and `crates/nomuxd/src/handlers/workspace.rs`.

## What survives the end of the daemon

| Event | Sessions |
|---|---|
| `nomux daemon stop`, `SIGTERM`, `SIGINT` | Continue |
| `kill -9` of the daemon, a daemon failure | Continue |
| `SIGKILL` for the process group of the daemon | Continue |
| The end of the terminal that started the daemon | Continue |
| A stop of a systemd unit with the default `KillMode=control-group` | **End** |
| A stop of a systemd unit with `KillMode=process` | Continue |

The pooled holders are not sessions: they end with the daemon in each case.

These properties make a holder independent of the daemon:

- The holder calls `setsid` at its start. It has its own session and its own process group, thus a signal for the group of the daemon does not reach it.
- The daemon gives the holder of a session no pipe. The standard input and the standard output of the holder are `/dev/null`. Only a pooled holder has a pipe as its standard input, and it ends with the daemon: see "Holder pool". The standard error is a file without a name that the daemon reads only if the holder ends before it is ready. A holder that writes after the end of the daemon does not fail.
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
Documentation=https://nomux-sh.github.io/docs/

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
- **The environment.** The service manager gives the daemon no `TERM` and, with launchd, no `LANG`. A session gets `TERM` and a character set of UTF-8 from nomux: see "Environment of a session". For a language, add `LANG` to the file.
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
| Resident memory with the status engine, idle daemon without a session (glibc) | 4.4 MB | < 10 MB |
| Resident memory with the status engine, idle daemon with 20 sessions, each with a status task (glibc) | 7.1 MB to 7.2 MB | < 10 MB |
| CPU time and context switches in 60 s, idle daemon with 20 sessions, each with a status task | 0 and 0 | 0 |
| The same, and 10 of the sessions reported a status with a Child work before the 60 s | 0 and 0 | 0 |
| CPU time and context switches in 30 s, idle daemon with 3 dormant sessions and 1 ended session | 0 and 0 | 0 |
| Start of the daemon to a complete `nomux ls --json`, 50 dormant sessions with a journal of 32 MiB each: median, slowest of 11 runs | 25.3 ms, 31.4 ms | < 1 s |
| Key to echo on the screen of `nomux attach`, local session: median, 99th percentile | 0.2 ms to 0.5 ms, 0.5 ms to 5.4 ms | < 10 ms |
| The same during a flood of 10 MB/s in a second attached session | 0.2 ms to 0.5 ms, 1.4 ms to 4.7 ms | no change that matters |
| The same during a flood without a limit in a second attached session (19 MB/s to 36 MB/s to its client) | 0.2 ms to 0.8 ms, 3.1 ms to 5.3 ms | no change that matters |
| Growth of the resident memory during a flood of 32 MiB to a client that takes no output | less than 8 MiB (the limit of the test) | no growth with the length of the output |
| `Session.Create` from the holder pool, connected client, debug build: median of 11 (the test `a_session_from_the_pool_opens_in_less_than_10_ms`) | 1.2 ms to 2.0 ms | < 10 ms |
| `Session.Create` without the pool, connected client: median, slowest of 50 | 2.31 ms, 2.43 ms | |
| The same, the part in the daemon: start of the holder process, wait for `ready`, lease, response | 0.12 ms to 0.19 ms, 2.2 ms, 0.08 ms, 0.03 ms | |
| Request to the first command of the shell in a directory that is not the home directory, `bash` with start files of 200 ms: without the pool, from the pool (median of 50) | 205.9 ms, 1.1 ms | < 10 ms |
| The same, `bash` with an empty start file | 2.7 ms, 1.1 ms | < 10 ms |
| `nomux new`, `bash` with an empty start file: without the pool, from the pool (median of 50) | 3.2 ms, 2.0 ms | < 10 ms |
| Resident memory of the daemon with a full pool of 2, idle (glibc) | 4.6 MB | < 10 MB |
| Resident memory of 2 pooled holders (glibc); one pooled holder (musl, static) | 5.6 MB; 0.8 MB to 0.9 MB | < 3 MB for each holder |
| Resident memory of the 2 shells of the pool (bash 5.3 with an empty start file) | 8.7 MB | no target |
| CPU time in 30 s of the idle daemon with a full pool, its pooled holders and their shells | 0 | 0 |

The rows of the holder pool are from the program `tests/measurements/new_session.rs` with the release build and `SHELL=/bin/bash`, with an empty `.bashrc` except where a row says more: [tests/README.md](../tests/README.md) says how to run it. The part in the daemon was measured with log lines that are not in the code. The shell with start files of 200 ms is `bash` with a `.bashrc` of `sleep 0.2`. From the pool, the shell ran its start files before the request, thus the time does not depend on them. For `bash` with an empty start file, the start of the CLI and its connection are the larger part of `nomux new`. For comparison, `bash -i` with the start files of a real user took 232 ms to its first prompt on the same computer, and its first prompt after a typed `cd` took 12.5 ms.

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
- **State.** The daemon keeps no database. The workspace tree is a file of its own: [workspace.md](workspace.md#storage). What it knows about a session after a restart comes from the holder and from the session state directory. The status of a session comes from the holder again, but `since` and `seen` start again, and the self-report and the event log are gone.
- **Status.** The status sources of agent tools (hooks, ACP), `wait`, and notifications to the operating system.
- **Session environment.** No part of the environment of a session comes from the client: a client with a different locale, or with variables such as `SSH_AUTH_SOCK` or `DISPLAY`, does not give them to its session. A list of variables that `Session.Create` carries from the client needs a field in the control protocol, and a rule for the holder pool: a pooled shell has the variables of no client.
- **Daemon update.** `nomuxd --replace` takes over from a daemon that runs, and the clients of that daemon lose their connection. An update that keeps the connections is not written.
- **Service files.** The launchd agent was not loaded on a Mac. The commands do not give the file to the service manager, and they do not stop a daemon.
- **Log rotation.** `nomuxd.log` has no size limit. With the default level it stays almost empty.
- **Holder pool.** The start files of a pooled shell run in the home directory. Only `bash`, `zsh` and `fish` get the directory of a session that is not in the home directory. A session with a command gets a new holder. The pool has no size for each shell or each directory.
- **Hibernation.** The memory pressure trigger on macOS, and a Task that is done as a candidate at once: [memory.md](memory.md#open-points).
- **Host identity, authentication, the QUIC transport.**
