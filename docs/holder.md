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
                               +--> screen state (VT parser, private memory)
                               +--> program reports (OSC parsers, private memory)
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
| `--rows <number>`, `--columns <number>` | 24, 80 | The size of the terminal window: 1048576 cells at most |
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

## Screen state

The holder keeps the current screen of the session: the visible cells with their attributes, the cursor and the terminal modes.
The state is a `nomux_term::Terminal` of the size of the terminal window. [`nomux-term`](../crates/nomux-term) is the abstraction over the VT library.

- The holder gives each chunk to the terminal in the step that writes the chunk to the ring buffer. Thus the screen state always has each byte that the ring buffer got, and no other byte.
- The terminal keeps no scrollback. The memory of the screen state depends only on the size of the window: approximately 50 bytes for each cell, because the VT library keeps the primary screen and the alternate screen.
- A window has 1048576 cells at most (`WindowSize::MAX_CELLS`). The holder does not start with a larger window, and it answers a larger `Resize` with `InvalidPayload`.
- `Resize` changes the size of the PTY and then the size of the screen state.
- The terminal gives the holder each OSC sequence that the VT library does not handle. "Program reports" shows what the holder keeps from them.
- After the command ended, the holder keeps the screen state. The daemon can ask for the last screen.

### Redraw sequence and its position

The redraw sequence is a byte sequence that makes the current screen in a new terminal of the same size. `Terminal::write_redraw` writes it. The documentation of that function lists the parts of the terminal state that the sequence does not make.

The holder gives the redraw sequence together with a position of the ring buffer. The rule for this pair is:

- The redraw sequence has the effect of each byte before the position.
- The redraw sequence has the effect of no byte from the position on.

Thus the daemon sends the redraw sequence to a client, and then the bytes of the ring buffer from the position. The client gets each byte one time: there is no gap and no repeat.

**The position is the start of the incomplete sequence.** The redraw sequence makes the screen, not the state of the VT parser. The holder reads the PTY in chunks of 4096 bytes, thus the write position is frequently in the middle of an escape sequence or of a UTF-8 character. The bytes of a sequence that is not complete did not change the screen. Thus:

- If the output ends in an escape sequence (CSI, OSC, DCS, APC, PM, SOS, or ESC with intermediate bytes) or in a UTF-8 character that is not complete, the position is the start of that sequence or character.
- If not, the position is the write position.

`Terminal::incomplete_tail` gives the length of the incomplete sequence. `nomux-term` follows the states of the parser of the VT library for this, and a test compares the two at each byte offset of its inputs.

One case is not exact. A control character (for example a line feed) in a CSI sequence or in an escape sequence has its effect immediately. A client that attaches between that control character and the end of the sequence gets the control character a second time. Programs do not write such sequences.

If an incomplete sequence is longer than the ring buffer, the position is older than the oldest byte of the ring buffer, and the reader of the daemon reports an overrun. The daemon then waits for output and asks again.

## Program reports

Programs and shells report their state in the terminal output. The holder reads these reports and keeps the last state in private memory. This state is the program reports of the session:

| Part | Source | What the holder keeps |
|---|---|---|
| Program status | OSC 7501, the [Program Status Protocol](https://control-codes.page/proposals/osc-7501/), revision 0.4 | One record for each `id`: the last report |
| Input line state | OSC 133, the prompt marks `A`, `B`, `C` and `D` | The last mark, the last exit code, and if input went to the terminal after the mark |
| Notifications | OSC 9 (the form of iTerm2) and OSC 777 (`notify`, the form of rxvt-unicode) | The last 16 notifications |

The holder keeps the program reports while it runs, also when no daemon is connected. A daemon that starts again reads them with the `ProgramReports` request.

The parsers are in `nomux-term`, in the modules of `nomux_term::osc`: one module for each code and one type for each parsed record. `Osc::record` selects the parser from the code. The holder changes the parsed records into the types of `nomux_holder_protocol::program_reports` in one module, `program_reports.rs`.

A report is untrusted input. Each program that writes to the terminal can write each byte sequence.

- `nomux-term` ignores an OSC sequence of more than 4096 bytes.
- Each field of a record has a size limit, and a parser allocates only the fields of one record.
- A report that is not valid is ignored. It does not stop the holder.
- A text for a person (`title`, `msg`, the title and the body of a notification) is one line of UTF-8 without a control character (U+0000 to U+001F, U+007F to U+009F). A report with a different text is ignored.

The holder gives each change of the program reports a number: the **revision**. The revision starts at 0 and only increases. Each record and each notification has the revision of its last change. A report that is ignored, and a `clear` that removes no record, do not change the revision.

### Program status

The holder obeys the rules of the specification for one report:

- A report is a list of `key=value` pairs with `:` between them. A pair that is not valid and a key that is not known are ignored. If a key repeats, the last value is used.
- `state` is necessary: `idle`, `working`, `done`, `blocked` or `error`. `state=clear` removes the record of the `id` and each record below it. Without an `id`, it removes all records.
- An `id` is a path: `build/test` is below `build`, and `build` is below the root record, which has no `id`. A record above a record does not have to exist.
- A report replaces its record fully. A key that the report does not have is not in the record after that.
- `kind` (`permission`, `question`, `auth`) applies only to `blocked`. `progress` (0 to 100) applies only to `working` and `blocked`.
- `title` and `msg` are base64 of UTF-8 text. The padding is optional.
- A report is discarded fully if it has no known `state`, if a key or a value is longer than its limit, if the `id` is not a path of names, or if `title` or `msg` does not decode to one line of text.

One rule is more strict than the text of the specification: an `id` with a byte that a value cannot have discards the report. If only the pair was ignored, a `clear` for one record removed all records.

| Item | Limit |
|---|---|
| Sequence | 4096 bytes |
| Key | 16 bytes |
| `id` | 128 bytes, 8 names, 32 bytes for each name |
| `app` | 32 bytes |
| `title` | 256 bytes of base64, 192 bytes of text |
| `msg` | 2732 bytes of base64, 2048 bytes of text |
| Records | 64. When a report makes one more record, the holder removes the record with the oldest report |

The specification permits a terminal to keep fewer than 256 records, and asks for 64 or more. The limits give a largest memory use of approximately 154 KiB for the records (64 records of 2400 bytes of text).

The holder stores the records and reads no meaning into them. These rules of the specification are not in the holder:

- **The life of a record.** The specification says that a terminal removes the `working` and `blocked` records when a new prompt starts (OSC 133 `A`) or when the process ends. The holder keeps them. The daemon applies this rule: it compares the revision of a record with the revision of the input line state, and it knows the end of the command from the `Status` request.
- **Full reset.** The specification says that RIS (`ESC c`) removes all records. The VT library handles RIS and does not tell the holder, thus the records stay.
- **The name that a record takes from a record above it.** The holder stores the `app` value of the report, or none. It uses the rule only for the forgery guard.
- **OSC 9;4.** The holder does not change a progress report of `ConEmu` into a program status record.

### The query

A tool sends program status reports only to a terminal that reads them. It asks with the query `OSC 7501 ; ? ST`. The holder answers on the input side of the PTY with the bytes of `nomux_term::osc::program_status::QUERY_REPLY`:

```
ESC ] 7501 ; ?:states=idle,working,done,blocked,error:kinds=permission,question,auth ESC \
```

The answer has no space. Revision 0.4 of the specification gives this form: `?`, then `states` with the states that the terminal accepts (without `clear`), then `kinds`.

- The answer always ends with `ESC \`, also when the query ended with BEL. The specification permits the two forms of ST.
- The answer goes through the input queue of the holder, as the bytes of an `Input` request do, and it needs no lease.
- While the input queue has 65536 bytes or more, the holder does not answer a query. Thus a program that sends queries and does not read its input cannot make the memory of the holder grow.
- The holder answers each program that asks. The forgery guard does not apply to the query, because the answer has no data of a report.

### Forgery guard

Each program that writes to the terminal can write a report with each `app` value: `cat` that prints a file, the output of a remote host in `ssh`, a log line. Without a check, such a report shows a blocked agent that does not exist. Thus the holder accepts a report only if the program in the foreground of the terminal agrees with the `app` value that the report claims.

**The claimed `app` value** is the `app` key of the report. If the report has no valid `app` key, it is the `app` value of the nearest record above its `id` that has one. This is the value that the record gets from the specification. The rule is the same for `state=clear`.

**The foreground program** is the leader of the foreground process group of the terminal:

| Step | Linux | macOS |
|---|---|---|
| Foreground process group | `tcgetpgrp` on the master side of the PTY (`TIOCGPGRP`) | The same |
| Name of the process whose ID is the ID of the group | Field 2 of `/proc/<pid>/stat`: 15 bytes at most | `proc_pidinfo` with `PROC_PIDTBSDINFO`: `pbi_name` (32 bytes at most), or `pbi_comm` if `pbi_name` is empty |

`nomux_holder_protocol::discovery::program_name` reads the name. It uses the code that reads the start time of a process for the discovery files: one read gives the two values.

**The two names agree** if they are equal, or if one of them is the other one with one of the characters `-`, `_`, `.`, `+` and more text after it. Upper case and lower case ASCII letters are equal.

| `app` | Program name | Result |
|---|---|---|
| `codex` | `codex` | Agree |
| `claude-code` | `claude` | Agree |
| `terraform` | `terraform.bin` | Agree |
| `claude-code` | `cat` | Do not agree |
| `claudette` | `claude` | Do not agree |

**The decision:**

| Report | Result |
|---|---|
| The claimed `app` value agrees with the foreground program | Accepted |
| The claimed `app` value does not agree, or the terminal has no foreground program with a name | Ignored: no record changes |
| No claimed `app` value | Accepted. The record has no `app` value, thus the daemon must not show it as the status of a known tool |

The holder reads the foreground program one time at most for each chunk of output (4096 bytes), and only if the chunk has a report with a claimed `app` value.

**Limits of the guard.** The guard stops a report in the output of a program that is not the tool that the report names. It is not a security boundary against the program that the user runs.

- **The same name.** A program can have each name: the name comes from the file name of the program, and a process can change it (`prctl(PR_SET_NAME)` on Linux). A program with the name `claude` can report as `claude-code`.
- **Interpreters.** A tool that runs in an interpreter has the name of the interpreter (`node`, `python3`) if it does not set its own name. Its reports with an `app` value are ignored. The native program of Claude Code has the name `claude` on Linux. The names of the other tools were not examined.
- **Only the leader of the group.** In `producer | tool`, the leader of the group is `producer`, and the reports of `tool` are ignored. A wrapper that starts the tool in its own process group without `exec` has the name of the wrapper.
- **Programs behind a program.** For a tool on a remote host or in a container, the foreground program is `ssh` or the container client. Its reports with an `app` value are ignored.
- **Time of the check.** The holder examines the foreground program when it reads the report, not when the program wrote it. A tool that writes `state=done` and ends immediately can be gone at that time: the report is then ignored. In the other direction, bytes that a program left in the PTY are examined against the program that started after it.
- **15 bytes on Linux.** If the name of a program is longer than 15 bytes, Linux gives its first 15 bytes. An `app` value agrees with it only by the rule above.
- **Removal.** The guard protects the name of a record, not the record. A report without a claimed `app` value can replace or remove a record: `state=clear` without an `id` and without an `app` key removes all records if the root record has no `app` value.

### Input line state

The shell integration of a shell writes the prompt marks. The holder keeps:

| Field | Value |
|---|---|
| Mark | The last mark: none, prompt start (`A`), input start (`B`), command start (`C`) or command end (`D`). The options after the letter are ignored |
| Exit code | The exit code of the last `D` mark, or none if that mark had no number. The other marks do not change it |
| Input after the mark | If the holder wrote bytes of an `Input` request to the terminal after the last mark |
| Revision | The revision of the last change of these fields |

- The input line is empty when the mark is input start and no input went to the terminal after it.
- A command runs when the mark is command start.
- Without a mark, the state is not known: the shell has no shell integration.

The holder does not read the screen to find the text of the input line. Thus the field "input after the mark" has these limits: bytes that the user typed before the mark (during the command before it) are in the input line and are not counted, and each byte of an `Input` request is counted, also a byte that is not a key (a reply of the terminal of a client). The answer to the query is not counted.

### Notifications

| Sequence | Title | Body |
|---|---|---|
| `OSC 9 ; body ST` | None | 2048 bytes at most |
| `OSC 777 ; notify ; title ; body ST` | 192 bytes at most | 2048 bytes at most. The body can have `;` |

- The holder keeps the last 16 notifications in the sequence of their arrival. A new notification removes the oldest one. The largest memory use is approximately 36 KiB.
- A notification that is too long, or that is not one line of UTF-8 text, is ignored.
- `ConEmu` uses OSC 9 for commands: `OSC 9 ; number ; ...` with a number from 1 to 12, for example `9;4` for a progress report. Such a sequence is not a notification. Thus a notification of the iTerm2 form whose body is one of these numbers, or starts with one of them and a `;`, is not read.
- The holder does not read OSC 99. It does not limit the rate of the notifications: the daemon decides what a notification causes.
- The forgery guard does not apply. A notification names no program.

## Control protocol

The control socket is a Unix stream socket. The protocol has its own version, which is independent of the product version and of the protocol between the clients and the daemon.
The current version is 1.2. "Protocol versions" shows what each version added.

The encoding is written by hand: fixed little-endian fields with a length prefix. The holder decodes 9 small messages, thus a schema compiler and its runtime give no benefit, and the binary stays small.

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
| `0x0006` | `Redraw` | Position of the ring buffer (8), length of the redraw sequence (4). Since version 1.1 |
| `0x0007` | `ProgramReports` | Revision of the program reports (8), length of the encoded program reports (4). Since version 1.2 |
| `0x0008` | `ProgramReportsChanged` | Revision of the program reports (8). Since version 1.2 |

The holder sends `Hello` when a connection opens, before the daemon sends a message. Thus the daemon knows the protocol version before its first request. The read-only descriptor of the ring buffer comes with the hello as `SCM_RIGHTS` ancillary data.

Three messages come with a descriptor: `Hello`, `Redraw` and `ProgramReports`. A descriptor arrives with the first byte of its message, thus the daemon reads the socket with `recvmsg` each time. While the daemon waits for the reply to a `Redraw` or a `ProgramReports` request, it sends no second request of these two kinds on that connection. Thus a descriptor that arrives after the hello always belongs to the next reply of these two kinds.

| Error code | Name | Meaning |
|---|---|---|
| 1 | `UnknownKind` | The holder does not know the request kind |
| 2 | `InvalidPayload` | The payload of the request is not valid |
| 3 | `LeaseRequired` | The request needs the lease, and this connection does not have it |
| 4 | `ChildExited` | The command has ended |
| 5 | `WaitPending` | This connection already has a wait request of this kind without a reply |
| 6 | `Failed` | The operating system refused the operation |
| 7 | `RedrawTooLarge` | The redraw sequence is longer than 8 MiB. Since version 1.1 |

### Messages from the daemon

The holder answers each request with one message that has the same request identifier.

| Kind | Request | Payload | Lease | Answer |
|---|---|---|---|---|
| `0x0101` | `AcquireLease` | Empty | | `Done` |
| `0x0102` | `Input` | The bytes for the terminal | necessary | `Done` |
| `0x0103` | `Resize` | Rows (2), columns (2). Not 0, and 1048576 cells at most | necessary | `Done` |
| `0x0104` | `Signal` | Signal (1): 1 `SIGHUP`, 2 `SIGINT`, 3 `SIGTERM`, 4 `SIGKILL` | necessary | `Done` |
| `0x0105` | `Status` | Empty | | `Status` |
| `0x0106` | `WaitOutput` | The position that the daemon already has (8) | | `Output`, possibly later |
| `0x0107` | `Redraw` | Empty. Since version 1.1 | | `Redraw` with a descriptor |
| `0x0108` | `ProgramReports` | Empty. Since version 1.2 | | `ProgramReports` with a descriptor |
| `0x0109` | `WaitProgramReports` | The revision that the daemon already has (8). Since version 1.2 | | `ProgramReportsChanged`, possibly later |

`Signal` sends the signal to the process of the command. After the holder has the exit status, it sends no signal, because the process ID can belong to a different process.

`Input` is complete when the holder has the bytes. If the terminal does not take them, the holder keeps them. While it keeps 65536 bytes or more, it reads no request from a connection.

### Redraw

`Redraw` gives the daemon what a client that attaches needs: the redraw sequence of the current screen and the position of the ring buffer that the screen corresponds to. "Redraw sequence and its position" has the rule for the pair. The request needs no lease.

The holder makes the sequence and the position in one step of its loop, between two chunks of output. Output that arrives while the holder serves the request is after the position.

The redraw sequence does not go into the frame:

- It can be longer than the 65536 bytes of a payload: each cell can have its own colors.
- The holder writes a reply with one `write` call and keeps no reply that the socket does not take. A send buffer of a Unix socket can be 8 KiB.

Thus the holder writes the sequence into new shared memory (made as the memory of the ring buffer) and sends the read-only descriptor of that memory with the `Redraw` reply. The memory has no header: the sequence starts at offset 0, and the reply has its length. The holder closes its descriptors immediately, thus the memory lives as long as the daemon keeps its descriptor or a mapping. `nomux_holder_protocol::redraw::read` copies the sequence out of the memory.

| Case | Answer |
|---|---|
| The screen has a redraw sequence of 8 MiB (`redraw::MAX_LENGTH`) or less | `Redraw` with the descriptor |
| The redraw sequence is longer | `Error` with `RedrawTooLarge`. The daemon can show only the bytes of the ring buffer |
| The holder cannot make the shared memory | `Error` with `Failed` |
| The command has ended | `Redraw`: the last screen |

A screen of 80 x 24 cells has a redraw sequence of some kilobytes. The sequence has approximately 80 bytes for a cell with all attributes and three RGB colors, and each combining character adds its bytes. A window of 400 x 120 cells in which each cell has a different full style gives approximately 4 MiB. Only a very large window with a different style in almost each cell is above the limit.

To attach a client, the daemon does these steps:

1. It sends `Redraw` and maps the memory of the reply.
2. It sends the redraw sequence to the client.
3. It moves its ring buffer reader to the position of the reply (`RingReader::seek`) and sends the bytes from there.

### Program reports

`ProgramReports` gives the daemon the program reports of the session: the program status records, the input line state and the notifications. "Program reports" shows what the holder keeps. The request needs no lease.

The encoded program reports can be longer than the payload of a frame: 64 records and 16 notifications with texts of the largest length have approximately 190 KiB. Thus they go to the daemon as a redraw sequence does: the holder writes them into new shared memory and sends the read-only descriptor with the reply. The memory has no header before the encoded bytes, and the reply has their length and their revision. `nomux_holder_protocol::program_reports::read` reads the memory, and refuses a length of more than 262144 bytes (`program_reports::MAX_LENGTH`).

All integers are little-endian. A block is a length (2) and that number of bytes. A text is a block of UTF-8, and a text that is absent is empty.

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | Header length: the number of bytes of the header after this field. 28 in this version |
| 4 | 8 | Revision of the program reports |
| 12 | 1 | Last prompt mark: 0 none, 1 prompt start, 2 input start, 3 command start, 4 command end |
| 13 | 1 | Flags: bit 0 input after the mark, bit 1 the exit code is present |
| 14 | 2 | Zero |
| 16 | 4 | Exit code of the last command (signed) |
| 20 | 8 | Revision of the input line state |
| 28 | 2 | Number of program status records |
| 30 | 2 | Number of notifications |
| 32 | | One block for each program status record, then one block for each notification. The oldest notification is first |

| Block | Fields |
|---|---|
| Program status record | Revision of the record (8). State (1): 1 idle, 2 working, 3 done, 4 blocked, 5 error. Kind (1): 0 none, 1 permission, 2 question, 3 auth. Progress (1): 0 to 100, or 255 for none. Zero (1). Four texts: `id` (empty for the root record), `app`, `title`, message |
| Notification | Revision of the notification (8). Two texts: title, body |

A later minor version adds fields at the end of the header or at the end of a block. A reader uses the header length and the block lengths to ignore the bytes that it does not know. A reader ignores a record whose state it does not know, and it reads a kind or a prompt mark that it does not know as none.

`WaitProgramReports` has the pattern of `WaitOutput`. The daemon sends the revision that it has. The holder answers `ProgramReportsChanged` when the revision is larger, or when the command has ended. Thus the daemon does not poll, and many changes between two requests give one answer.

| Case | Answer |
|---|---|
| The revision is larger than the revision of the request | `ProgramReportsChanged` immediately |
| The revision is the same | `ProgramReportsChanged` after the next change, or when the command ends |
| The command has ended | `ProgramReportsChanged` immediately. The program reports do not change after that, thus the daemon sends no more wait request |
| The connection already has a `WaitProgramReports` request without a reply | `Error` with `WaitPending` |
| The holder cannot make the shared memory for `ProgramReports` | `Error` with `Failed` |

A connection can have one `WaitOutput` request and one `WaitProgramReports` request without a reply at the same time.

To follow the program reports, the daemon does these steps:

1. It sends `ProgramReports` and reads the memory of the reply.
2. It sends `WaitProgramReports` with the revision of the reply.
3. After `ProgramReportsChanged`, it does step 1 again.

A notification that has a revision larger than the revision of the last read is new. If more than 16 notifications arrive between two reads, the daemon gets the last 16.

### Compatibility rules

A holder that runs is never replaced. A daemon must talk to each holder of the same major version, also to one that is much older.

- The frame header and the first four bytes of the hello payload (the protocol version) never change.
- A change in a major version is an addition: a new message kind, a new error code, or new fields at the end of a payload. Each addition increases the minor version.
- A receiver ignores the bytes of a payload that come after the fields that it knows.
- A holder answers a request kind that it does not know with `UnknownKind`. The connection stays open.
- The daemon reads the minor version from the hello and sends only the requests that this version has.
- If the major version in the hello is different, the daemon reads no other field.

The tests in `crates/nomux-holder-protocol/tests/wire.rs` keep the bytes of the frames and these rules. The test in `crates/nomux-holder/tests/holder/compatibility.rs` sends a request kind of a later version to a real holder.

### How to add to the protocol

The minor version is the only signal of what a holder can do. The hello has no capability flags: a holder has each addition of its minor version and of the versions before it, thus one number says all. Capability flags are necessary only for functions that a holder can have or not have independently, and a holder has no such function.

To add a request, a reply, an error code or a field, do these steps:

1. Increase the minor version in `ProtocolVersion::CURRENT`.
2. For a new request, give the new minor version in `Request::minor_version`. `ProtocolVersion::supports` then tells the daemon if a holder knows the request.
3. Use the next free kind or error code. Never use a kind or a code again, and never change the fields of a payload that exists: add fields only at its end.
4. Add a row to "Protocol versions" and the words "Since version" to each new row of the message tables.
5. Add the bytes of the new frames to the tests in `wire.rs`.
6. Write in this document what the daemon does when a holder is too old for the addition.

The daemon must work with a holder that does not have the addition. It asks `ProtocolVersion::supports` before it sends a new request. If it sends the request without this check, an old holder answers `UnknownKind` and the connection stays open: that reply is defined, and it is not a reason to close the connection.
An old daemon never sees a new reply kind or a new error code, because only a new request gives them. If it gets an error code that it does not know, it reads it as `Unrecognized`.

### Protocol versions

| Version | Addition | A daemon with an older holder |
|---|---|---|
| 1.0 | The first version: all messages without a "Since" note | |
| 1.1 | Request `Redraw`, reply `Redraw`, error code `RedrawTooLarge` | It cannot get the screen. It shows a client only the bytes that the ring buffer has |
| 1.2 | Requests `ProgramReports` and `WaitProgramReports`, replies `ProgramReports` and `ProgramReportsChanged` | It has no program status, no input line state and no notification from that holder. A holder of version 1.1 does not answer the OSC 7501 query, thus the tools in its session send no program status report |

No release has a holder of version 1.0 or 1.1, thus each holder that runs has version 1.2 or later.

The types `Request`, `Reply` and `ErrorCode` of `nomux-holder-protocol` are `#[non_exhaustive]`. Thus an addition does not stop the build of code that reads them.

### Connections and the single-writer lease

- The socket has mode 0600 in a directory with mode 0700. The holder also reads the user ID of the peer (`SO_PEERCRED` on Linux, `getpeereid` on macOS) and closes a connection from a different user.
- The holder accepts 4 connections at one time. It closes a fifth connection immediately.
- The holder continues when no connection is open, and it accepts a new connection after a disconnect.
- Each connection can read: `Status`, `WaitOutput`, `Redraw`, `ProgramReports` and `WaitProgramReports` need no lease.
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
| `nomux-holder-protocol`, `mapping.rs`: `Mapping::map`, `atomic`, `copy_out`, `slice_mut`, `drop` | `mmap`, `munmap` and access to the shared memory of the ring buffer, of a redraw sequence and of the program reports |
| `nomux-holder-protocol`, `discovery/process_record.rs` (macOS only) | `proc_pidinfo` has no safe interface |
| `nomux-holder`, `child.rs`: `take_controlling_terminal` | `Command::pre_exec`: `setsid` and `TIOCSCTTY` between `fork` and `exec` |
| `nomux-holder`, `control/peer.rs` (macOS only) | `getpeereid` has no safe interface |

## Measurements

Measured on Linux x86-64 with the release profile. The targets are in [bench/README.md](../bench/README.md).

| Measurement | Result | Target |
|---|---|---|
| Size of `nomux-holder` (glibc, dynamic) | 560 KB | < 1 MB |
| Size of `nomux-holder` (musl, static) | 652 KB | < 1 MB |
| Resident memory, idle holder with `/bin/sh` (glibc) | 2.6 MB | < 3 MB |
| Resident memory, ring buffer full (glibc) | 2.9 MB | < 3 MB |
| Resident memory, idle holder with `/bin/sh` (musl, static) | 0.7 MB | < 3 MB |
| Resident memory, ring buffer full (musl, static) | 1.0 MB | < 3 MB |
| Resident memory, ring buffer full and program reports full (musl, static) | 1.3 MB | < 3 MB |
| Resident memory, ring buffer full and program reports full (glibc) | 3.2 MB | < 3 MB: not met |
| CPU time and context switches in 10 s, idle | 0 and 0 | 0 |
| Output of `cat` of a 128 MiB file through the holder, plain text | 71 MiB/s | |
| Output of `cat` of a 128 MiB file through the holder, colored words | 84 MiB/s | |
| Output of `cat` of a 32 MiB file through the holder, a program status report in each line | 54 MiB/s | |

"Program reports full" is the largest state that a program can make: 64 program status records and 16 notifications, each with texts of the largest length. It adds approximately 0.3 MB. A session with the reports of a real tool has some records with short texts, and its memory is the memory of the rows without "program reports full". The release archives have the static musl binary.

The screen state adds 134 KB to the binary and approximately 0.2 MB to the resident memory of a window of 80 x 24 cells. The program reports add 22 KB to the glibc binary and 16 KB to the musl binary.
During a flood of output, the VT parser uses one third to one half of the CPU time of the holder, and the kernel uses the other part.

The three throughput values are the best of 11 runs on a host that did other work at the same time. The median is 10 % to 15 % lower. With the same method and at the same time, the holder without the program reports gives 68 MiB/s, 82 MiB/s and 65 MiB/s. Thus the program reports do not change the throughput of output without reports: the difference is smaller than the difference between two sets of runs. Output with a report in each line is 17 % slower, because the holder decodes each report, examines the foreground program one time for each chunk, and replaces the record.

A ring buffer of 1 MiB gives more than 3 MB of resident memory when it is full. This is the reason for the default of 256 KiB.

## Open points for the next versions

These parts are not in the holder yet:

- **Parts of the terminal state that the redraw sequence does not make.** The scroll region, the tab stops, the saved cursor, the character sets, the hyperlinks and the changed palette colors. The VT library gives no read access to most of them.
- **Journal and checkpoint.** There is no epoch and no sequence number. A daemon that connects gets the current screen and the bytes that the ring buffer still has.
- **Program reports.** The holder does not see RIS, thus a full reset does not remove the program status records. The forgery guard reads only the name of the leader of the foreground process group: it ignores the reports of a tool that runs in an interpreter, behind a pipe or on a remote host. The holder does not read OSC 99, and it does not read the text of the input line from the screen.
- **Fuzz target.** The parsers of `nomux_term::osc` have no fuzz target yet.
- **Scrollback on disk.**
- **Warm pool.** There is no message that gives a session to a holder that already runs.
- **Lease notification.** The connection that loses the lease learns it from the answer to its next write request, not from a message.
- **Send across threads.** `RingReader` is not `Send`. The daemon decides how it shares a reader between tasks.
- **Compatibility tests.** When a release has a holder, CI must test the new daemon code with a holder of each older version.
- **Performance tests.** `bench` has no job that measures the targets of this document on each pull request.
