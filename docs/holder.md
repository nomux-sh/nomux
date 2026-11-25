# Holder

This document describes the session holder (`nomux-holder`) and its contract with the daemon.
The code of the holder is in `crates/nomux-holder`. The contract is in `crates/nomux-holder-protocol`, which the holder and the daemon share.

[architecture.md](architecture.md) shows where the holder is in the core.

## Process model

One holder owns the PTY of one session. The holder is one process with one thread and no async runtime.

```
starter (daemon) --exec--> nomux-holder --fork, exec--> command of the session
                               |  ^                          |
                               |  +------ PTY master --------+ PTY slave: controlling terminal,
                               |                               standard input, output and error
                               +--> ring buffer (shared memory) --> daemon reads
                               +--> control socket <--------------> daemon connects
                               +--> discovery files
```

The starter runs:

```sh
nomux-holder --session <identifier> [options] -- <command> [argument...]
```

| Option | Default | Function |
|---|---|---|
| `--session <identifier>` | necessary | The session identifier: 1 to 64 characters from `A-Z`, `a-z`, `0-9`, `-` and `_` |
| `--runtime-directory <path>` | `~/.nomux/run` | The directory for the discovery files |
| `--working-directory <path>` | the current directory | The working directory of the command |
| `--rows <number>`, `--columns <number>` | 24, 80 | The size of the terminal window |
| `--ring-capacity <bytes>` | 262144 | The size of the ring buffer: a power of two, 4096 or more |
| `--exit-timeout <seconds>` | 30 | The time to keep the exit status for the daemon |

The command gets the environment of the holder. There is no option for environment variables, because the arguments of a process are visible to other users and the environment is not. The starter sets the environment of the holder.

At the start, the holder does these steps in this sequence:

1. It calls `setsid`. Thus a signal for the terminal or for the process group of the starter does not reach the holder.
2. It installs a handler for `SIGCHLD` that writes one byte to a pipe.
3. It makes the shared memory of the ring buffer.
4. It makes the discovery directory and writes `pid`.
5. It makes the control socket `sock`.
6. It opens the PTY and starts the command. The command is the leader of a new session, and the PTY is its controlling terminal.
7. It writes `ready`.

Then the holder waits in one `poll` call for these descriptors: the `SIGCHLD` pipe, the listening socket, the PTY and each connection.
`poll` has no timeout while the command runs. An idle holder makes no system call and uses no CPU time.

The holder uses `rustix` for the system calls. It uses `signal-hook` for the `SIGCHLD` handler, because the end of a child process has no file descriptor that is the same on Linux and macOS.

## Ring buffer

The ring buffer carries the terminal output from the holder to the daemon. It is one region of shared memory of a fixed size.
The holder reads each chunk from the PTY directly into the region. The daemon maps the region read-only and copies bytes out of it. A read needs no system call and no copy in the kernel.

| Host | Shared memory |
|---|---|
| Linux | `memfd_create`. The read-only descriptor comes from a second `open` of `/proc/self/fd/<n>` with `O_RDONLY` |
| macOS | `shm_open` with `O_CREAT \| O_EXCL`, a second `shm_open` with `O_RDONLY`, then `shm_unlink`. The memory has no name after that |

The holder sends the read-only descriptor to the daemon with the hello message. The kernel permits only a read-only mapping of this descriptor.

### Layout

All integers are little-endian.

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | Magic: `NMXRING\0` |
| 8 | 4 | Layout version: 1 |
| 12 | 4 | Data offset: where the data starts. 64 in this version |
| 16 | 8 | Capacity: the number of data bytes, a power of two |
| 24 | 8 | Reserved, zero |
| 32 | 8 | Claimed position (atomic) |
| 40 | 8 | Committed position (atomic) |
| 48 | 16 | Reserved, zero |
| 64 | capacity | Data |

A reader gets the length of the region from `fstat` and the start of the data from the data offset field. Thus a later version can make the header longer.

### Positions and the wrap rule

A position is the number of bytes that the holder wrote since it started. A position only increases. The byte at position `p` is at data index `p mod capacity`.

- The **committed** position is the end of the bytes that a reader can read.
- The **claimed** position is the end of the bytes that the writer can change.

To write a chunk, the writer does these steps:

1. It sets the claimed position to the end of the chunk. A chunk has 4096 bytes at most and does not go over the end of the data. If the chunk reaches the end of the data, the next chunk starts at data index 0.
2. It reads from the PTY into the chunk.
3. It sets the committed position, and then the claimed position, to the end of the bytes that it read.

The bytes from `claimed - capacity` to `committed` are stable.

The writer keeps its position in private memory and does not read the shared region. A process that damages the region cannot stop the holder.

### Overrun

A reader keeps its own position `r`. A new reader starts at `claimed - capacity`, or at 0 if that is negative: the oldest byte that the ring buffer has.

To read, the reader does these steps:

1. It loads the committed position. If `committed - r` is larger than the capacity, the reader is overrun.
2. It copies the bytes from `r` to the committed position into its own buffer. The copy wraps at the end of the data.
3. It loads the claimed position. If `claimed - r` is larger than the capacity, the writer was able to change the bytes during the copy: the reader is overrun.

After an overrun, the reader discards the copy, continues at the committed position and reports the number of bytes that it lost.

The copy in step 2 is necessary. A reader cannot give a reference into the shared memory to other code, because the holder can write there at each moment.

### How the daemon waits for output

The daemon does not poll the ring buffer. It sends a `WaitOutput` request with its read position. The holder answers when the committed position is larger, or when the command has ended. Thus the holder writes to the socket one time for each wait, not one time for each chunk, and a daemon that is slow gets fewer answers.

## Control protocol

The control socket is a Unix stream socket. The protocol has its own version, which is independent of the product version and of the protocol between the clients and the daemon.
The current version is 1.0.

The encoding is written by hand: fixed little-endian fields with a length prefix. The holder decodes 6 small messages, thus a schema compiler and its runtime give no benefit, and the binary stays small.

### Frame

Each message is one frame.

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | Length: the number of bytes after this field (8 + payload) |
| 4 | 2 | Kind |
| 6 | 2 | Flags: the sender writes 0, the receiver ignores the value |
| 8 | 4 | Request identifier |
| 12 | n | Payload: 65536 bytes at most |

If the length is smaller than 8 or the payload is longer than 65536 bytes, the receiver closes the connection.

### Messages from the holder

| Kind | Message | Payload |
|---|---|---|
| `0x0001` | `Hello` | Protocol major version (2), minor version (2), process ID of the command (4), rows (2), columns (2), length of the session identifier (1), session identifier |
| `0x0002` | `Done` | Empty |
| `0x0003` | `Error` | Error code (2) |
| `0x0004` | `Status` | State (1): 0 running, 1 exited, 2 killed by a signal. 3 bytes of zero. Exit code or signal number (4) |
| `0x0005` | `Output` | Committed position of the ring buffer (8) |

The holder sends `Hello` when a connection opens, before the daemon sends a message. Thus the daemon knows the protocol version before its first request. The read-only descriptor of the ring buffer comes with the hello as `SCM_RIGHTS` ancillary data.

| Error code | Name | Meaning |
|---|---|---|
| 1 | `UnknownKind` | The holder does not know the request kind |
| 2 | `InvalidPayload` | The payload of the request is not valid |
| 3 | `LeaseRequired` | The request needs the lease, and this connection does not have it |
| 4 | `ChildExited` | The command has ended |
| 5 | `WaitPending` | This connection already waits for output |
| 6 | `Failed` | The operating system refused the operation |

### Messages from the daemon

The holder answers each request with one message that has the same request identifier.

| Kind | Request | Payload | Lease | Answer |
|---|---|---|---|---|
| `0x0101` | `AcquireLease` | Empty | | `Done` |
| `0x0102` | `Input` | The bytes for the terminal | necessary | `Done` |
| `0x0103` | `Resize` | Rows (2), columns (2). Not 0 | necessary | `Done` |
| `0x0104` | `Signal` | Signal (1): 1 `SIGHUP`, 2 `SIGINT`, 3 `SIGTERM`, 4 `SIGKILL` | necessary | `Done` |
| `0x0105` | `Status` | Empty | | `Status` |
| `0x0106` | `WaitOutput` | The position that the daemon already has (8) | | `Output`, possibly later |

`Signal` sends the signal to the process of the command. After the holder has the exit status, it sends no signal, because the process ID can belong to a different process.

`Input` is complete when the holder has the bytes. If the terminal does not take them, the holder keeps them. While it keeps 65536 bytes or more, it reads no request from a connection.

### Compatibility rules

A holder that runs is never replaced. A daemon must talk to each holder of the same major version, also to one that is much older.

- The frame header and the first four bytes of the hello payload (the protocol version) never change.
- A change in a major version is an addition: a new message kind, a new error code, or new fields at the end of a payload. Each addition increases the minor version.
- A receiver ignores the bytes of a payload that come after the fields that it knows.
- A holder answers a request kind that it does not know with `UnknownKind`. The connection stays open.
- The daemon reads the minor version from the hello and sends only the requests that this version has.
- If the major version in the hello is different, the daemon reads no other field.

The tests in `crates/nomux-holder-protocol/tests/wire.rs` keep the bytes of the frames and these rules.

### Connections and the single-writer lease

- The socket has mode 0600 in a directory with mode 0700. The holder also reads the user ID of the peer (`SO_PEERCRED` on Linux, `getpeereid` on macOS) and closes a connection from a different user.
- The holder accepts 4 connections at one time. It closes a fifth connection immediately.
- The holder continues when no connection is open, and it accepts a new connection after a disconnect.
- Each connection can read: `Status` and `WaitOutput` need no lease.
- One connection at most has the lease. Only this connection can send `Input`, `Resize` and `Signal`. Other connections get `LeaseRequired`.
- `AcquireLease` always moves the lease to the connection that asks. The connection that had the lease gets `LeaseRequired` for its next write request.
- A disconnect releases the lease.

The last request wins because a new daemon must be able to take the lease from an old daemon that does not answer. The lease prevents two writers at the same time. It is not a security boundary: only the same user can connect.

## Discovery files

Each holder has one directory in the runtime directory of the user. The name of the directory is the session identifier.

```
~/.nomux/run/            runtime directory, mode 0700
  <session identifier>/  holder directory, mode 0700
    pid                  "<process ID> <start time>\n"
    sock                 control socket, mode 0600
    ready                empty; the holder writes it last
```

The start time tells a holder from a later process that got the same process ID.

| Host | Source of the start time | Unit |
|---|---|---|
| Linux | Field 22 of `/proc/<pid>/stat` | Clock ticks after the system start |
| macOS | `proc_pidinfo` with `PROC_PIDTBSDINFO`: `pbi_start_tvsec` and `pbi_start_tvusec` | Microseconds after the Unix epoch |

The value is compared only for equality.

`nomux_holder_protocol::discovery::scan` reads a runtime directory and gives the state of each holder directory:

| State | Condition |
|---|---|
| `Live` | The process with the recorded process ID runs, has the recorded start time, and `ready` exists |
| `Starting` | The same, but `ready` does not exist |
| `Stale(ProcessGone)` | No process has the recorded process ID, or the process is a zombie |
| `Stale(ProcessReplaced)` | A process has the recorded process ID with a different start time |
| `Stale(InvalidPidFile)` | `pid` does not exist or cannot be parsed |

The daemon connects to the `Live` holders and removes the `Stale` directories with `DiscoveredHolder::remove_directory`.

The holder does not start if a directory for its session exists. The daemon removes a stale directory before it starts a holder with the same session identifier.

A holder makes its directory and then writes `pid`. A scan between these two steps reports `Stale(InvalidPidFile)`. If the daemon removes the directory then, the holder cannot make its socket and stops with an error.

## Lifecycle

1. **Start.** The holder does the steps of "Process model". If a step fails, the holder removes its directory and exits with status 1. A usage error gives status 2.
2. **Run.** The holder moves output from the PTY to the ring buffer, and answers requests. If all processes close the PTY but the command continues, the holder stops reading the PTY and waits for the end of the command.
3. **End of the command.** `SIGCHLD` wakes the holder. It collects the exit status, reads the output that is in the PTY (256 chunks at most), closes the PTY and answers each `WaitOutput` request. The close of the PTY sends `SIGHUP` to the processes that remain in the session.
4. **Exit status.** The holder keeps answering requests. `Status` gives the exit status. `Input`, `Resize` and `Signal` give `ChildExited`.
5. **Exit.** When a connection got the exit status, or when the exit timeout passes, the holder removes its discovery directory and exits with status 0.

If the holder is killed, the directory stays. The scan reports it as stale.

## Unsafe code

The workspace denies `unsafe`. These items permit it with `#[expect(unsafe_code)]`:

| Item | Reason |
|---|---|
| `nomux-holder-protocol`, `ring/mapping.rs`: `Mapping::map`, `atomic`, `copy_out`, `slice_mut`, `drop` | `mmap`, `munmap` and access to the shared memory |
| `nomux-holder-protocol`, `discovery/process_start_time.rs` (macOS only) | `proc_pidinfo` has no safe interface |
| `nomux-holder`, `child.rs`: `take_controlling_terminal` | `Command::pre_exec`: `setsid` and `TIOCSCTTY` between `fork` and `exec` |
| `nomux-holder`, `control/peer.rs` (macOS only) | `getpeereid` has no safe interface |

## Measurements

Measured on Linux x86-64 with the release profile. The targets are in [bench/README.md](../bench/README.md).

| Measurement | Result | Target |
|---|---|---|
| Size of `nomux-holder` (glibc, dynamic) | 404 KB | < 1 MB |
| Size of `nomux-holder` (musl, static) | 500 KB | < 1 MB |
| Resident memory, idle holder with `/bin/sh` (glibc) | 2.2 MB | < 3 MB |
| Resident memory, ring buffer full (glibc) | 2.5 MB | < 3 MB |
| Resident memory, idle holder with `/bin/sh` (musl, static) | 0.4 MB | < 3 MB |
| CPU time and context switches in 10 s, idle | 0 and 0 | 0 |

A ring buffer of 1 MiB gives 3.5 MB of resident memory when it is full. This is the reason for the default of 256 KiB.

## Open points for the next versions

These parts are not in the holder yet:

- **VT state.** The holder does not parse the output. A reader that is overrun has no screen state to continue from.
- **Journal and checkpoint.** There is no epoch and no sequence number. A daemon that connects gets only the bytes that the ring buffer still has.
- **OSC parsing.** OSC 7501, 133, 9 and 777 are not read. Thus the holder does not know the state of the input line.
- **Scrollback on disk.**
- **Warm pool.** There is no message that gives a session to a holder that already runs.
- **Lease notification.** The connection that loses the lease learns it from the answer to its next write request, not from a message.
- **Send across threads.** `RingReader` is not `Send`. The daemon decides how it shares a reader between tasks.
- **Compatibility tests.** When version 1.1 exists, CI must test the new daemon code with a holder of each older version.
- **Performance tests.** `bench` has no job that measures the targets of this document on each pull request.
