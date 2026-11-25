# Extension host

This document describes the extension host: how the daemon keeps the installed extensions, how it runs the daemon part of an extension as a separate process, and which limits it applies.
The code is in `crates/nomux-ext-host`. The daemon does not use the crate yet.

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

`ExtensionHost::install` installs a package from a directory:

1. It validates `extension.toml` with the manifest validator of `nomux-extension`.
2. It refuses a package whose `api` requirement does not match the extension API version of the host.
3. It refuses a package that does not have the program of its `[daemon]` table.
4. It refuses approved permissions that the manifest does not ask for.
5. It copies the package. A package can have only regular files and directories: the host refuses a symbolic link, because a link can point out of the package.
6. It writes the state file and enables the extension.

The state file is not in the package, thus a package cannot bring its own approval.

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

`ExtensionHost::shutdown` stops each process in the same way. The daemon calls it before it stops.

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

## Not done yet

- **File access restriction.** The process has the file access of the daemon. The restriction of the process to the approved `fs` directories with Landlock on Linux is the next work. The sandbox on macOS is not decided.
- **Enforcement of `process` and `network`.** The host gives these permissions to the extension in `Hello` and does not enforce them.
- **Daemon wiring.** `nomuxd` does not make an `ExtensionHost`, the control protocol has no `Ext` service, and the CLI has no `nomux ext` commands.
- **Host services.** The daemon supplies no implementation of `HostServices`.
- **Installation from the registry, and signature checks.** The host installs only from a local directory.
- **Update and removal** of an installed extension.
- **`lazy = false`.** The host starts each extension at its first call.
- **`${worktree}`.** The host does not replace the variable. The caller of `install` gives the approved directories as absolute paths.
- **Processes that an extension starts.** When the host ends a process, the processes that the extension started continue to run.
- **Event subscriptions for each extension,** and a limit for the rate of the protocol messages of an extension.
- **UI parts** of an extension.
