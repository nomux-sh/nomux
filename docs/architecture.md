# Architecture of the core

This document shows the components of the nomux core and the responsibility of each component.
It is an overview. The protocol specifications are in the [`proto`](https://github.com/nomux-sh/proto) repository.

The holder, the daemon, the client library and the CLI have their first version: [holder.md](holder.md), [daemon.md](daemon.md) and [cli.md](cli.md) describe them. The other components are not implemented yet.
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

The first four responsibilities and the discovery files are implemented. [holder.md](holder.md) has the details and the parts that are open.

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

The first version creates, lists and ends sessions, finds the holders again after a restart and takes their lease. [daemon.md](daemon.md) has the details and the parts that are open.

- **Sessions:** Find the holders from their discovery files and connect to them, also after a daemon restart. Take the single-writer lease. Send terminal data to the clients. Keep a small pool of holders that already have a shell (warm pool), thus a new session starts immediately.
- **State:** Keep the data model, the status resolver and the event log in SQLite (WAL mode).
- **Extensions:** Start the daemon part of each extension as a separate process when a client needs it. Apply the permissions of the extension.
- **Events:** Send events to the clients and to the notification channels.
- **Host:** Keep the host identity, authenticate the clients, and update the daemon without stopping a session.
- **Restore after reboot:** Restore all workspaces as metadata only. Start the process of a session when a client opens its tab for the first time.

Rules:

- One daemon runs for each user and each runtime directory. A second daemon ends with an error.
- The daemon does not own a PTY. Thus a daemon failure stops no session.
- Terminal data does not go through the component that owns the state.
- The daemon calculates data such as metrics or git status only while a client watches it.

## Client (`nomux-client`, `nomux`)

The client library `nomux-client` is the client side of the protocol. The CLI, the desktop client and the mobile clients use it.

Responsibilities:

The first version connects to the daemon of the local host, starts it when it does not run, does the handshake and sends the requests for sessions and for the daemon. [cli.md](cli.md) has the commands of the CLI.

- Connect to a daemon through a transport: a Unix socket on the local host, SSH or QUIC for a remote host.
- Do the handshake: protocol version, oldest supported version and capability flags.
- Send numbered control requests and receive the replies and the events.
- Receive terminal data in one of two modes. If the client keeps up, it gets the raw bytes. If the client is behind, it gets only the latest state of the screen.
- Send input on a separate channel that has the highest priority.
- After a disconnect, attach again with the epoch and the last sequence number.

Rules:

- A client talks to the daemon only. It does not talk to a holder.
- The CLI (`nomux`) contains no protocol code. It reads the command line, calls `nomux-client` and prints the result.

## Crates

| Crate | Type | Responsibility | State |
|---|---|---|---|
| `nomux-core` | library | Shared types, errors, configuration, glossary types | in use: the version output of the binaries, the lookup of a binary of the same installation |
| `nomux-term` | library | Thin abstraction over the VT library (`alacritty_terminal`), which lets the project replace the library | in use: the screen state, the redraw sequence and the OSC sequences |
| `nomux-holder-protocol` | library | Contract between a holder and the daemon: control messages, ring buffer layout, discovery files | in use by the holder |
| `nomux-holder` | binary | The holder. No async runtime | owns the PTY, the ring buffer, the control socket and the discovery files |
| `nomux-transport` | library | Unix socket, SSH channels, QUIC streams, credit windows | in use: the Unix socket of the local host and the framing of the control channel |
| `nomux-status` | library | Status resolver, rules, aggregation. Pure code without I/O | structure only |
| `nomux-ext-host` | library | Extension host of the daemon | structure only |
| `nomuxd` | binary | The daemon | creates, lists and ends sessions; finds the holders after a restart |
| `nomux-client` | library | Protocol client for the CLI, the desktop client and the mobile clients | in use: connection, handshake and the requests for sessions and for the daemon |
| `nomux-cli` | binary `nomux` | The CLI | `new`, `ls`, `kill`, `daemon status`, `daemon stop`, `version` |
| `nomux-end-to-end` (in `tests`) | tests | End-to-end tests with the three binaries | in use |

Planned direction of the dependencies:

- `nomux-core` depends on no other nomux crate.
- `nomux-holder-protocol` depends on no other nomux crate. It has no async runtime.
- `nomux-holder` uses `nomux-core`, `nomux-holder-protocol` and `nomux-term`. It must not use a crate that needs an async runtime.
- `nomux-transport` depends on no other nomux crate of this repository. It uses the messages of the `nomux-proto` crate, which the `proto` repository publishes.
- `nomuxd` uses `nomux-core`, `nomux-holder-protocol`, `nomux-term`, `nomux-transport`, `nomux-status` and `nomux-ext-host`. It finds the holders and talks to them only through `nomux-holder-protocol`.
- `nomux-client` uses `nomux-core`, `nomux-term` and `nomux-transport`.
- `nomux-cli` uses `nomux-client` and `nomux-core`. It uses no type of `nomux-proto`.

Now the holder depends on `nomux-core` and `nomux-holder-protocol`. The daemon depends on `nomux-core`, `nomux-holder-protocol` and `nomux-transport`. The client library depends on `nomux-core` and `nomux-transport`, and the CLI depends on the client library and `nomux-core`. `nomux-term` depends on the VT library, and no crate uses `nomux-term` yet. `nomux-status` and `nomux-ext-host` have no dependencies and no users.
A crate gets a dependency when its code uses it.

Two definitions exist in two crates, because the client side must not depend on the holder contract: the runtime directory `~/.nomux/run` is in `nomux-holder-protocol` (for the holders) and in `nomux-transport` (for the socket of the daemon). The daemon uses the first one for the two purposes, and an end-to-end test fails if the client looks for the socket in a different directory.

## State

The repository has the Cargo workspace, the developer tools and CI. The holder runs a command in a PTY and serves the daemon side of its contract. The daemon starts holders, finds them again after a restart and serves the clients on a Unix socket. The CLI creates, lists and ends sessions.

The work comes in this sequence:

1. The rest of the holder (VT state, journal, checkpoint, OSC parser), attach, the warm pool, lazy restore, the state database of the daemon and the selection of the VT library.
2. Remote hosts: the SSH transport and the bootstrap.
3. The extension host.
4. The agent status engine.
5. The QUIC transport and port forwarding.
6. The `uniffi` feature of `nomux-client` for the mobile clients.

The [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md) has the plan of the full project.
