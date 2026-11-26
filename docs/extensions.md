# Extension host

This document describes the extension host: how the daemon keeps the installed extensions, how it runs the daemon part of an extension as a separate process, and which limits it applies.
The code of the host is in `crates/nomux-ext-host`. The daemon embeds one host: "The host in the daemon" says where its directory is, how a client installs and calls an extension, and which host services the daemon supplies. The code of that part is in `crates/nomuxd/src/extensions` and `crates/nomuxd/src/handlers/ext.rs`.

[architecture.md](architecture.md) shows where the extensions are in the core. The [extension protocol](https://github.com/nomux-sh/extension-api/blob/main/spec/extension-protocol.md) specifies the messages, and [ADR-0005](https://github.com/nomux-sh/docs/blob/main/adr/0005-extension-model.md) records the decision to run the daemon part as a separate process.

In this document, the **host** is the crate `nomux-ext-host` in the daemon, and the **extension** is the daemon part of an extension.

## Host model

The crate is a library. The daemon makes one `ExtensionHost` and gives it two things: a directory, and its host services as an implementation of the `HostServices` trait.

```
nomuxd
+-- ExtensionHost ----------------------------------------------+
| registry    : packages, state files (no process, no task)     |
| supervisor  : one for each extension that got a call          |      stdin: frames from the host
|   +-- task of the connection (only while a process runs) -----+----> process of the extension
|         calls, events, health requests, idle time, stderr log |<---- stdout: frames, stderr: log text
+---------------------------------------------------------------+
```

- The **registry** reads the installed extensions from the directory. It starts nothing.
- A **supervisor** owns the life cycle of one extension across its processes: the start on the first call, the pause after a failure, and the decision to disable the extension. The host makes the supervisor of an extension at its first call.
- A **connection** is one process of an extension, from its start to its end. One task owns the process, its three pipes and the state of the protocol. The task ends with the process.

The messages, the framing constants and the manifest validator come from the crate `nomux-extension`, which the `extension-api` repository publishes. The host has its own reader for the frames, because `nomux-extension` reads from a blocking stream and the host reads from an asynchronous stream.

### Cost of an extension that is not in use

An installed extension without a call has no process, no task and no timer. Its cost is one entry in the registry.
`ExtensionHost::open` reads the directory and starts no task. A process that is idle for `idle` stops, and its task ends.

The test `the_first_call_starts_the_process_and_an_idle_process_stops` examines this: the runtime of the test has 0 tasks after the installation, 1 task while the process runs, and 0 tasks after the idle stop. Each timer of the host is in the task of a connection or in a call that waits for its result.

## Installed extensions

The directory of the host has three parts:

```
<directory>/
  packages/<id>/     the package of an extension, with its extension.toml
  state/<id>.toml    the state file of the extension
  data/<id>/         the working directory of the process
```

An installation has two steps, thus the user sees the permissions of an extension before the host has the extension.

`ExtensionHost::inspect` is the first step. It reads a package in a directory and changes nothing:

1. It validates `extension.toml` with the manifest validator of `nomux-extension`.
2. It refuses a package whose `api` requirement does not match the extension API version of the host.
3. It refuses a package that does not have the program of its `[daemon]` table.

The result is the manifest, which has the permissions that the extension asks for.

`ExtensionHost::install` is the second step. It gets the directory and the permissions that the user approved:

1. It does the checks of the first step again.
2. It refuses approved permissions that the manifest does not ask for.
3. It copies the package. A package can have only regular files and directories: the host refuses a symbolic link, because a link can point out of the package.
4. It writes the state file and enables the extension.

An installation starts no process. The extension gets the approved permissions and no others, also when its manifest asks for more.
The state file is not in the package, thus a package cannot bring its own approval.

The copy of a package can take time. During the copy the host does not hold the lock of its registry: the other functions of the host do not wait. A second installation of the same identifier during that time is refused.

`ExtensionHost::remove` stops the process of an extension and removes its package, its state file and its working directory. First the host moves the two directories to names that it does not read (`packages/.removed-<id>`, `data/.removed-<id>`), then it deletes them. Thus a call or a new installation of the identifier never sees a part of the old files. A host that ended during the deletion leaves these directories, and the next removal of the identifier deletes them.

### State file

The host owns the state file. The format is TOML:

```toml
enabled = false
disabled_reason = "the user disabled the extension"

[permissions]
fs = ["/home/user/project"]
process = ["git"]
network = false
scopes = ["sessions:read"]
```

| Key | Content |
|---|---|
| `enabled` | `true` if the host starts the extension. The key is necessary. |
| `disabled_reason` | Why the extension is disabled. Only when `enabled` is `false`. |
| `permissions.fs` | The directories that the user approved, as absolute paths. |
| `permissions.process` | The programs that the user approved. |
| `permissions.network` | `true` if the user approved network connections. |
| `permissions.scopes` | The scopes of host services that the user approved. |

A key of `[permissions]` that is not in the file is an empty list or `false`. A key that this format does not have makes the file not valid. The host writes the file to a temporary name and renames it.

The approved permissions are a part of what the manifest asks for. An approved directory is equal to an item of `fs` in the manifest, or the item starts with `${worktree}` and the directory ends with the rest of the item.

When the daemon starts, the host reads each package again. A package that is not valid, or that has no valid state file, is not in the registry, and the host writes a warning to the log. One bad package does not stop the daemon.

## Life cycle of a process

```
                    call                    Welcome, InitializeResponse
   +---------+ ---------------> +----------+ ------------------------> +---------+
   | Stopped |                  | Starting |                           | Running |
   +---------+ <--------------- +----------+                           +---------+
        ^      idle or stop:         |                                      |
        |      ShutdownResponse,     | failure                              | failure
        |      exit status 0         v                                      |
        |                       +--------+ <--------------------------------+
        +---------------------- | Failed | --- call after the pause ---> Starting
          the user enables      +--------+
          the extension again        |
                                     | too many failures in sequence, or a failure that a new
                                     v start cannot correct
                                 disabled (in the state file)
```

`ExtensionHost::extension` reports the state of the process (`RunState`) and the enabled state (`Enablement`).

### Start

The first call of a method of an extension starts its process. The manifest key `lazy` has no effect yet.

1. The host does not start an extension that is disabled, that has no daemon part, whose `api` requirement does not match the extension API version of the host, or whose pause after a failure did not end. The caller gets the reason.
2. The host starts the program of the `[daemon]` table.
3. The host sends `Hello` with its extension API version, the identifier of the manifest and the approved permissions.
4. The host examines `Welcome`. The identifier must be the identifier of the manifest, and the version that the program was built for must match the `api` requirement of the manifest.
5. The host sends `InitializeRequest`. Until the response, the extension can call host services and send events.

Calls that arrive during a start wait for it. They use the same process.

The process gets:

- **A clean environment.** It gets only the variables whose names are in `HostConfig::inherited_variables`, with the values of the daemon. The default is `PATH` and `HOME`.
- **Its own working directory:** `data/<id>`. The host makes the directory.
- **Three pipes.** The process does not get the terminal of the daemon.

### Calls from the host

`ExtensionHost::call` sends a `CallRequest` and waits for the response.

- A call that has no response after `call` fails with `Timeout`, and the host sends `Cancel`.
- To cancel a call, the caller drops the future of the call. The host sends `Cancel`.
- After `Cancel`, the extension must answer the call in `cancel`. An extension runs its methods one at a time, thus a method that does not stop holds each later call.
- A call that is larger than one frame fails before the host sends it.
- If the process fails, each call that waits gets the failure.

### Calls from the extension

The extension calls a host service with a `CallRequest`. The host asks the embedder for the scope of the method (`HostServices::required_scope`) and answers:

| Condition | Answer |
|---|---|
| The embedder does not offer the method. | `ERROR_CODE_NOT_FOUND` |
| The user did not approve the scope of the method. | `ERROR_CODE_PERMISSION_DENIED` |
| The extension already has 64 calls that run. | `ERROR_CODE_FAILED` |
| The result does not go into one frame. | `ERROR_CODE_FAILED` |
| The extension sends `Cancel` for the call. | The host stops the work and answers `ERROR_CODE_CANCELLED`. |

The host has no host service of its own. This crate applies only the scopes: the other permissions (`fs`, `process`, `network`) go to the extension in `Hello`, and the host does not enforce them yet.

### Events

`ExtensionHost::subscribe` gives the events of all the extensions. Each event has the identifier of its extension, thus two extensions can use the same event name.
A subscriber that is slow loses the oldest events. An event without a subscriber is dropped.

### Idle stop

A process is idle when no call runs in the two directions. An event that a subscriber gets counts as activity: the host does not stop an extension whose events are in use.
After `idle` without activity, the host sends `ShutdownRequest`. The process must answer and end with the status 0 in `shutdown`. If it does not, the host ends it, and this is a failure.
The subsequent call starts a new process.

`ExtensionHost::shutdown` stops each process in the same way. The daemon calls it before it stops. After it, the host starts no process: a call fails with `Stopped`. Thus a request that arrives during the stop of the daemon does not leave a process behind.

## Isolation

A failure of an extension stays in its process. The daemon and the sessions continue, and the other extensions continue to answer.
For each failure in this table, the host ends the process, fails the calls that wait, and records the failure as the state of the extension:

| What the extension does | Failure |
|---|---|
| Its program does not start. | `Spawn` |
| Its process ends without a shutdown request, with each exit status or signal. | `Exited` |
| It closes its standard output and continues to run. | `ClosedOutput` |
| It does not complete the handshake and the initialization in `start`. | `StartTimeout` |
| It answers `InitializeRequest` with an error. | `InitializeRefused` |
| It does not answer a health request in `health`. | `HealthTimeout` |
| It does not take a frame from its standard input in `health`. | `InputBlocked` |
| It does not answer a cancelled call in `cancel`. | `CancelTimeout` |
| It does not end in `shutdown` after `ShutdownRequest`. | `ShutdownTimeout` |
| It sends bytes that are not a frame. | `BadFrame` |
| It sends a frame that is larger than 4 MiB. | `OversizedFrame` |
| It sends a message before `Welcome`, or a result of the wrong type. | `Protocol` |
| It writes more than `stderr_flood_bytes` to its standard error in `stderr_flood_window`. | `StderrFlood` |
| It refuses the extension API version of the host, or its program was built for a version that its manifest does not name. | `VersionIncompatible` |
| Its program has a different identifier than its manifest. | `IdentityMismatch` |

While a process runs, the host sends a `HealthRequest` each `health_interval`. An extension answers it also while a method runs.

### Start after a failure

- After a failure, the host does not start the extension immediately. A call during the pause fails with `BackingOff` and starts nothing. The first call after the pause starts a new process.
- The pause is `restart_delay_first` after the first failure. Each failure in sequence makes it two times longer, up to `restart_delay_max`.
- After `failures_before_disable` failures in sequence, the host disables the extension and writes the last failure as the reason to the state file. `VersionIncompatible` and `IdentityMismatch` disable the extension at the first failure, because a new start gives the same result.
- A stop without a failure (idle stop or shutdown) sets the count of failures to 0.
- `ExtensionHost::enable` enables the extension again and sets the count to 0.

### Standard error

The host reads the standard error of each process and keeps the last `stderr_log_bytes` bytes for each extension. `ExtensionHost::stderr_log` gives them.
The host never copies these bytes to the output of the daemon. The log of the daemon gets one line for each failure, with the identifier of the extension.

## Limits

The extension protocol sets the size of a frame. The other limits are values of the host, in `Limits`:

| Limit | Field | Default |
|---|---|---|
| Size of one frame | (protocol) | 4 MiB |
| Start: process start to `InitializeResponse` | `start` | 10 s |
| One call from the host | `call` | 30 s |
| Answer to a cancelled call | `cancel` | 5 s |
| Pause between two health requests | `health_interval` | 10 s |
| Answer to a health request; time to take one frame | `health` | 5 s |
| Idle time before the stop | `idle` | 60 s |
| `ShutdownRequest` to the end of the process | `shutdown` | 3 s |
| First pause after a failure | `restart_delay_first` | 1 s |
| Longest pause after a failure | `restart_delay_max` | 60 s |
| Failures in sequence before the host disables the extension | `failures_before_disable` | 5 |
| Kept bytes of the standard error | `stderr_log_bytes` | 64 KiB |
| Bytes of standard error in one window | `stderr_flood_bytes` | 1 MiB |
| Window of the standard error limit | `stderr_flood_window` | 10 s |
| Events that the host keeps for a slow subscriber | `event_buffer` | 64 |
| Calls of host services of one extension at the same time | (constant) | 64 |

With the defaults, the pauses after failures are 1 s, 2 s, 4 s and 8 s, and the fifth failure disables the extension.

## The host in the daemon

`nomuxd` makes one `ExtensionHost` at its start, after it has the sessions of an earlier daemon. This costs one read of the directory of the host. The daemon starts no extension then, and it has no task and no timer for extensions while no extension is in use: "Cost of an extension that is not in use" has the reason. [daemon.md](daemon.md) has the measurement of the idle daemon.

### Directory

| Host | Directory of the extension host |
|---|---|
| Linux | `~/.local/share/nomux/extensions` |
| macOS | `~/Library/Application Support/nomux/extensions` |

The path comes from `HOME` only, as the path of the runtime directory does. The daemon makes the directory with mode 0700 and sets that mode at each start: the state files have the approvals of the user, and other users must not read or change them.
The directory is not the runtime directory and not the state directory of the sessions: an installed extension stays after a restart of the host, and a removal of the session state does not remove it.

### Ext service

The clients use the extensions through the `Ext` service of the control protocol. [`spec/control.md`](https://github.com/nomux-sh/proto/blob/main/spec/control.md) of the `proto` repository specifies it.

| Request | What the daemon does |
|---|---|
| `Ext.List` | `ExtensionHost::extensions`: each extension with its state, the reason for the state and the approved permissions |
| `Ext.Inspect` | `ExtensionHost::inspect` for a directory of the host: the identity of the package and the permissions that its manifest asks for |
| `Ext.Install` | `ExtensionHost::install` with the permissions of the request |
| `Ext.Remove` | `ExtensionHost::remove` |
| `Ext.SetEnabled` | `ExtensionHost::enable`, or `ExtensionHost::disable` with the reason `the user disabled the extension` |
| `Ext.Call` | `ExtensionHost::call`. The first call of an extension starts its process |

**No extension runs before the user approved its permissions.** The daemon starts a process only for `Ext.Call`, and only for an extension that `Ext.Install` installed. `Ext.Install` records the permissions of its request and no others. A client shows the answer of `Ext.Inspect` to the user and sends `Ext.Install` only with the approval of the user. The CLI gets the approval from its arguments: [cli.md](cli.md) describes `nomux ext install`.

An extension has one state for a client:

| State | Condition |
|---|---|
| `DISABLED` | The extension is disabled in its state file. The reason is the `disabled_reason` of the file |
| `STOPPED`, `STARTING`, `RUNNING` | The extension is enabled, and its process has this `RunState` |
| `FAILED` | The extension is enabled, and its last process failed. The reason is the failure |

The answer to `Ext.Call` separates two kinds of failure. If the extension answered the call with an error, the client gets that error with the error code of the extension protocol. If the extension gave no answer, the client gets an error of the control protocol: the code 2 for an extension that is not installed, the code 3 for a call that the extension did not answer in `call`, and the code 1 for each other reason, for example an extension that is disabled or a process that failed.
An envelope of the control protocol has 1 MiB at most, and a frame of the extension protocol has 4 MiB at most. The daemon answers a result that does not go into an envelope with the code 1.

### Host services

The daemon implements `HostServices` in `crates/nomuxd/src/extensions/host_services.rs`. An extension calls a service with a `CallRequest`, and the host answers only if the user approved the scope of the service: "Calls from the extension" has the rule.

| Method | Scope | Payload of the call | Payload of the result |
|---|---|---|---|
| `sessions.list` | `sessions:read` | Not read | A `SessionListResponse` message of the control protocol: the sessions of the host with their identifier, their state, the process ID of their command and their description (the command, the working directory, the window size, the start time, the epoch and the restore policy). The list has the dormant sessions and no ended session. It has no environment variable |

This is the only host service. The daemon gets a new service together with the official extension that needs it, and each service gets a scope of its own in this table.

### Isolation from the sessions

The host runs in the one thread of the daemon. The work of the host for a process is small: it reads and writes frames and waits for timers.

- A call of an extension is a request of one client connection. The daemon answers the requests of one connection in their sequence, thus a call that waits delays only the next request of the same connection. A session request or an attach on a different connection does not wait.
- An extension that hangs, fails or writes too much gets the treatment of "Isolation". The daemon and the sessions continue.
- `Ext.Inspect` and `Ext.Install` read and copy files, and `Ext.Remove` deletes files. This work runs on a different thread (`tokio::task::spawn_blocking`). The thread exists only during the work.

The end-to-end test `a_client_attaches_to_a_session_while_a_call_of_an_extension_that_hangs_waits` attaches to a session and types a command while a call of an extension that answers nothing waits.

### Idle time

The variable `NOMUX_EXTENSION_IDLE_SECONDS` of the daemon sets the `idle` limit: the time without activity after which the host stops a process. The value is a number of seconds from 1 to 86400, and the default is 60. With a value that is not valid, the daemon does not start.
The other limits have no setting.

### Stop of the daemon

When the daemon stops, it calls `ExtensionHost::shutdown` after it stopped accepting connections and before it waits for the requests that began. Each process gets `ShutdownRequest` and has the `shutdown` time (3 s) to end. A process that does not end in that time is ended by the host. A call that waits for an extension gets the code 1 then.
If the daemon ends without its stop sequence (`kill -9`), each process of an extension gets the end of its standard input. An extension that uses the extension SDK ends then.

## Not done yet

- **File access restriction.** The process has the file access of the daemon. The restriction of the process to the approved `fs` directories with Landlock on Linux is the next work. The sandbox on macOS is not decided.
- **Enforcement of `process` and `network`.** The host gives these permissions to the extension in `Hello` and does not enforce them.
- **Host services.** The daemon supplies only `sessions.list`.
- **Installation from the registry, and signature checks.** The host installs only from a local directory.
- **Update** of an installed extension: remove it and install the new version.
- **Events for the clients.** The control protocol has no event of an extension, thus no client subscribes to `ExtensionHost::subscribe` yet.
- **`lazy = false`.** The host starts each extension at its first call.
- **`${worktree}`.** The host does not replace the variable. The caller of `install` gives the approved directories as absolute paths.
- **Processes that an extension starts.** When the host ends a process, the processes that the extension started continue to run.
- **Event subscriptions for each extension,** and a limit for the rate of the protocol messages of an extension.
- **UI parts** of an extension.
