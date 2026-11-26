# Architecture of the core

This document shows the components of the nomux core and the responsibility of each component.
It is an overview. The protocol specifications are in the [`proto`](https://github.com/nomux-sh/proto) repository.

The holder, the daemon, the client library and the CLI have their first version, with attach, with remote hosts through SSH, with the journal of each session, with the restore of a session after a restart of the host, with extensions, with the agent status engine and with the agent adapters: [holder.md](holder.md), [daemon.md](daemon.md), [cli.md](cli.md), [remote.md](remote.md), [extensions.md](extensions.md), [status.md](status.md) and [adapters.md](adapters.md) describe them. The other components are not implemented yet.
The "State" section shows what exists and where the plan is.

## Components

```
[Desktop client]     [CLI: nomux]     [Mobile clients]
        \                 |                 /
         +---------- nomux protocol -------+
     (control: protobuf; terminal: binary frames; capability handshake)
                          |
   Transport: SSH (separate channels) | QUIC (separate streams) | Unix socket (local)
                          v
+------------------------------ nomuxd (one on each host) ----------------------------+
| sessions   : finds and manages the holders; sends stream or sync data to clients   |
| state      : data model, status resolver, event log (SQLite, WAL)                  |
| extensions : starts the daemon part of each extension; applies permissions         |
| events     : event bus to the clients and to the notification channels             |
| host       : identity, authentication, self-update (no session stops)              |
+------+---------------------------------------------------------+-------------------+
       | Unix socket (control) + memfd ring buffer (data)        | stdio, framed protobuf
+------v--------------+  +------------------+          +---------v--------------------+
| nomux-holder        |  | nomux-holder     |  ...     | extensions: git, fs,         |
| PTY, VT state,      |  |                  |          | agent-*, notify-*, usage ... |
| OSC parser,         |  |                  |          | (separate process, started   |
| journal, checkpoint |  |                  |          | only when necessary)         |
+---------------------+  +------------------+          +------------------------------+
```

There are three process types in the core: the holder, the daemon and the client.
The extensions are separate processes. They are not part of this repository.

## Holder (`nomux-holder`)

One holder owns the PTY of one session. The holder is a separate, small binary.

Responsibilities:

These responsibilities are implemented: the PTY and the command, the read of the PTY, the VT parser and the ring buffer, the discovery files and the single-writer lease. The holder gives the daemon the current screen as a redraw sequence. The OSC parser, the journal, the checkpoint and the scrollback are not implemented. [holder.md](holder.md) has the details and the parts that are open.

- Open the PTY and start the command of the session.
- Always read the PTY, also when no client is connected.
- Give the output to the VT parser, to the OSC parser (OSC 7501, 133, 9, 777) and to a ring buffer of fixed size. The ring buffer is shared memory (`memfd`). The daemon maps it and reads it without a copy.
- Write the screen changes to a sequential journal, and write a checkpoint at intervals. Each session has an epoch and a sequence number. A client that connects again sends "epoch E, last sequence N" and gets only the part that it does not have.
- Compress the long scrollback with zstd and write it to disk.
- Write the discovery files in `~/.nomux/run/<id>/`: `pid` (the process ID and the process start time), `sock`, and `ready`. The holder writes `ready` last.
- Accept one writer at a time (single-writer lease). During a daemon update, the old daemon and the new daemon cannot write to the holder at the same time.
- Know the state of the input line (OSC 133 B and C). The delivery of team messages uses this state.

Rules:

- The holder has no async runtime. It uses `rustix` (`openpt`, `poll`, `memfd`, `mmap`) directly.
- The holder stays alive when the daemon stops, crashes, gets `kill -9` or is updated.
- The holder does not talk to clients. Only the daemon connects to a holder.
- The protocol between the holder and the daemon has the strictest version rules. A daemon update does not replace a holder that runs.

## Daemon (`nomuxd`)

One daemon runs on each host.

Responsibilities:

The first version creates, lists and ends sessions, finds the holders again after a restart, takes their lease and attaches clients to the terminal of a session. It gives a client the output of the journal that the ring buffer has no more, and the scrollback. It keeps a session whose holder is gone as a dormant session, and it starts the command of that session again when a client opens it. It installs extensions from a directory, starts the daemon part of an extension at its first call and stops it when it is idle. It keeps the status of each session from its sources, with one task for each session that waits on the holder, and it sends the changes as events to the clients that subscribe. It takes the hook events of the agent tools, gives them to the adapter extension of the tool, takes the status reports of the adapters as the first source, and sends the events of the extensions to the clients. [daemon.md](daemon.md) and [extensions.md](extensions.md) have the details and the parts that are open.

- **Sessions:** Find the holders from their discovery files and connect to them, also after a daemon restart. Take the single-writer lease. Send terminal data to the clients. Keep a small pool of holders that already have a shell (warm pool), thus a new session starts immediately.
- **State:** Keep the data model, the status resolver and the event log in SQLite (WAL mode).
- **Extensions:** Start the daemon part of each extension as a separate process when a client needs it. Apply the permissions of the extension.
- **Events:** Send events to the clients and to the notification channels.
- **Host:** Keep the host identity, authenticate the clients, and update the daemon without stopping a session.
- **Restore after reboot:** Restore all workspaces as metadata only. Start the process of a session when a client opens its tab for the first time. The daemon does this for sessions: a session without a process is dormant, and each session has a restore policy (`auto`, `on-focus`, `manual`). The restore of a workspace layout is not written.

Rules:

- One daemon runs for each user and each runtime directory. A second daemon ends with an error.
- The daemon does not own a PTY. Thus a daemon failure stops no session.
- Terminal data does not go through the component that owns the state. An attached client has its own reader of the ring buffer of the holder, and the session list is used only to find the session.
- The daemon calculates data such as metrics or git status only while a client watches it.

## Client (`nomux-client`, `nomux`)

The client library `nomux-client` is the client side of the protocol. The CLI, the desktop client and the mobile clients use it.

Responsibilities:

The first version connects to the daemon of the local host or of a remote host through SSH, starts the daemon when it does not run, does the handshake, sends the requests for sessions, for extensions and for the daemon, and attaches to a session. For a remote host it also installs the binaries of its own version. An attach is a stream of terminal output and a sink for input: the library has no terminal handling. An attach that loses its connection connects again and continues from its epoch and sequence number, also when the output between is only in the journal or when the session started again. The library also reads the scrollback of a session and starts a dormant session. The second send mode and the QUIC transport are not implemented. [cli.md](cli.md) has the commands of the CLI, and [remote.md](remote.md) describes the SSH transport.

- Connect to a daemon through a transport: a Unix socket on the local host, SSH or QUIC for a remote host.
- Do the handshake: protocol version, oldest supported version and capability flags.
- Send numbered control requests and receive the replies and the events.
- Receive terminal data in one of two modes. If the client keeps up, it gets the raw bytes. If the client is behind, it gets only the latest state of the screen.
- Send input on a separate channel that has the highest priority.
- After a disconnect, attach again with the epoch and the last sequence number.
- Connect again with a limited number of attempts and a pause that grows. Show the user that a connection is lost.

Rules:

- A client talks to the daemon only. It does not talk to a holder.
- The CLI (`nomux`) contains no protocol code. It reads the command line, calls `nomux-client` and prints the result. For `attach` it also owns the terminal of the user: raw mode, the keys and the window size. For `ext validate` it checks a manifest with the validator of the extension SDK, without a daemon.

## Crates

| Crate | Type | Responsibility | State |
|---|---|---|---|
| `nomux-core` | library | Shared types, errors, configuration, glossary types | in use: the version output of the binaries, the lookup of a binary of the same installation, the global configuration file: the hosts, the default restore policy and the retention time of an ended session, the data directory of nomux, and the rules of a hook event |
| `nomux-term` | library | Thin abstraction over the VT library (`alacritty_terminal`), which lets the project replace the library | in use by the holder: the screen state, the redraw sequence and its position, and the OSC sequences |
| `nomux-holder-protocol` | library | Contract between a holder and the daemon: control messages, ring buffer layout, redraw memory, discovery files | in use by the holder |
| `nomux-holder` | binary | The holder. No async runtime | owns the PTY, the ring buffer, the screen state, the control socket and the discovery files |
| `nomux-transport` | library | Unix socket, SSH channels, QUIC streams, credit windows | in use: the Unix socket of the local host, the SSH client with its channels to the socket of a remote daemon, the framing of the control channel and the framing of the terminal channels of an attach |
| `nomux-status` | library | Status resolver, rules, aggregation. Pure code without I/O | in use by the daemon: the status model, the source sequence, the Child work tree, the aggregation, the rule for a session without a process, and the adapter source with its rules for a soft working state. [status.md](status.md) describes it |
| `nomux-ext-host` | library | Extension host of the daemon | in use by the daemon: the installed extensions, a process for each extension in use, calls, events and the isolation of failures. [extensions.md](extensions.md) describes it |
| `nomuxd` | binary | The daemon | creates, lists and ends sessions; finds the holders after a restart; attaches clients to a session; reads the journal and the state directory of a session; keeps dormant and ended sessions and starts a dormant session again; installs, removes and calls extensions; supplies the host services `sessions.list`, `agent.report` and `agent.child`; takes the hook events of the agent tools and gives them to their adapters; sends the events of the extensions to the clients; measures the memory of the processes of each session when a client asks; keeps the status of each session and the event log |
| `nomux-client` | library | Protocol client for the CLI, the desktop client and the mobile clients | in use: the local host and remote hosts, the bootstrap of a remote host, the handshake, the requests for sessions, for extensions, for the status of the sessions, for the hooks and for the daemon, the events of a host, the attach with its reconnect, the scrollback, the checks of a host, and the install of the hooks of a tool in its settings file |
| `nomux-cli` | binary `nomux` | The CLI | `new`, `ls` (with `--memory`), `attach`, `start`, `capture`, `restore-policy`, `detach`, `kill`, `daemon status`, `daemon stop`, `daemon install-service`, `daemon uninstall-service`, `host add`, `host ls`, `host rm`, `host doctor`, `ext ls`, `ext install`, `ext rm`, `ext enable`, `ext disable`, `ext call`, `ext validate`, `status`, `status set`, `agent explain`, `events --follow`, `hook`, `hooks install`, `hooks uninstall`, `hooks status`, `version`, and the `--host` option |
| `nomux-end-to-end` (in `tests`) | tests | End-to-end tests with the three binaries | in use |

Planned direction of the dependencies:

- `nomux-core` depends on no other nomux crate.
- `nomux-holder-protocol` depends on no other nomux crate. It has no async runtime.
- `nomux-holder` uses `nomux-core`, `nomux-holder-protocol` and `nomux-term`. It must not use a crate that needs an async runtime.
- `nomux-transport` depends on no other nomux crate of this repository. It uses the messages of the `nomux-proto` crate, which the `proto` repository publishes.
- `nomuxd` uses `nomux-core`, `nomux-holder-protocol`, `nomux-term`, `nomux-transport`, `nomux-status` and `nomux-ext-host`. It finds the holders and talks to them only through `nomux-holder-protocol`.
- `nomux-client` uses `nomux-core`, `nomux-term` and `nomux-transport`.
- `nomux-cli` uses `nomux-client` and `nomux-core`. It uses no type of `nomux-proto`. It uses the manifest validator of the `nomux-extension` crate.

Now the holder depends on `nomux-core`, `nomux-holder-protocol` and `nomux-term`. The daemon depends on `nomux-core`, `nomux-holder-protocol`, `nomux-transport`, `nomux-status` and `nomux-ext-host`. The client library depends on `nomux-core` and `nomux-transport`, and the CLI depends on the client library, `nomux-core` and, for the check of a manifest, `nomux-extension`. The client library does not use `nomux-term` yet: it gives the bytes of the terminal to its user. `nomux-term` depends on the VT library. `nomux-ext-host` depends on the `nomux-extension` crate, which the `extension-api` repository publishes. The daemon is its only user. `nomux-status` has no dependencies, and the daemon is its only user.
`nomux-transport` contains the SSH library (`russh`), which uses the `tokio` runtime. Only the client side calls it: the daemon uses the socket and the framing of the crate, and the release build removes the SSH code from `nomuxd`. `nomux-core` has a TOML library for the configuration file and no runtime.
A crate gets a dependency when its code uses it.

Two definitions exist in two crates, because the client side must not depend on the holder contract: the runtime directory `~/.nomux/run` is in `nomux-holder-protocol` (for the holders) and in `nomux-transport` (for the socket of the daemon). The daemon uses the first one for the two purposes, and an end-to-end test fails if the client looks for the socket in a different directory.

## State

The repository has the Cargo workspace, the developer tools and CI. The holder runs a command in a PTY and serves the daemon side of its contract. The daemon starts holders, finds them again after a restart and serves the clients on a Unix socket. The CLI creates, lists and ends sessions, and it attaches a terminal to a session and detaches it, on the local host and on a remote host through SSH. The daemon runs the daemon parts of the extensions that the user installed from a directory, and the CLI installs, lists, removes and calls them.

The work comes in this sequence:

1. The warm pool, the restore of a workspace layout, the state database of the daemon and the selection of the VT library.
2. The rest of the remote hosts: a second way for SSH servers and configurations that the SSH client does not support, and the local echo for a slow connection.
3. The rest of the extension host: the restriction of the file access of an extension, more host services, the installation from a registry, and the UI parts.
4. The agent status engine: the real adapters of the agent tools in the `extensions` repository, the answer of a hook with a decision, ACP, `wait`, and the notifications.
5. The QUIC transport and port forwarding.
6. The `uniffi` feature of `nomux-client` for the mobile clients.

The [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md) has the plan of the full project.
