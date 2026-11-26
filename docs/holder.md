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
                               +--> journal and session metadata (files) --> daemon reads
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
| `--runtime-directory <path>` | `~/.nomux/run` | The directory for the discovery files. The daemon gives the path of a short link if this path is too long for the sockets: see "Discovery files" |
| `--state-directory <path>` | See "Session state directory" | The directory for the journal and the session metadata |
| `--working-directory <path>` | the current directory | The working directory of the command |
| `--rows <number>`, `--columns <number>` | 24, 80 | The size of the terminal window: 1 row or more, 2 columns or more, and 1048576 cells at most |
| `--ring-capacity <bytes>` | 262144 | The size of the ring buffer: a power of two, 4096 or more |
| `--exit-timeout <seconds>` | 30 | The time to keep the exit status for the daemon |
| `--pooled` | off | Start as a pooled holder: the command runs, and the session comes later. See "Pooled holder" |
| `--launcher <word>` | none | One word of a program that starts the command; the option is repeated for each word. The holder starts the launcher with the command after its words, and the session metadata has the command. The daemon uses it for the memory limit of a session: `systemd-run --user --scope ... --`, which starts the command in its place with `exec`. [memory.md](memory.md#memory-limit) has the rule |

The command gets the environment of the holder with the changes of "Environment of the command". There is no option for environment variables, because the arguments of a process are visible to other users and the environment is not. The starter sets the environment of the holder.

### Environment of the command

The holder changes the environment of the command with the rule of [daemon.md](daemon.md#environment-of-a-session): it sets `TERM=xterm-256color` and `COLORTERM=truecolor`, a character set of UTF-8 if it has no locale, and removes `COLUMNS`, `LINES`, `TERM_PROGRAM` and `TERM_PROGRAM_VERSION`. It also adds three variables of the session. A program in the session uses them to tell the daemon which session it is in, for example `nomux status set` ([status.md](status.md#self-report-and-the-token)).

| Variable | Value |
|---|---|
| `NOMUX_SESSION_ID` | The session identifier |
| `NOMUX_SOCK` | The socket of the daemon: `nomuxd.sock` in the runtime directory of the holder |
| `NOMUX_TOKEN` | The token of this start of the holder: 32 lower-case hexadecimal characters, from 128 random bits of the operating system (`getrandom`) |

- The holder makes a new token at each start. Thus a restore of a session gives it a new token, and the token of the earlier process proves nothing.
- The token is only in the memory of the holder and in the environment of the command. No file has it: not the discovery files, not the session metadata and not the journal. Its `Debug` form does not show it.
- The daemon gets the token with the `SessionToken` request on the private socket. It compares a token of a request in a time that does not depend on the first byte that is different.
- The processes of the session inherit the variables. Thus each process of the session can prove that it is in the session. A process of the same user outside the session can read the environment of a process of the session: the token is not a defence against it, as the socket is not.

At the start, the holder does these steps in this sequence:

1. It calls `setsid`. Thus a signal for the terminal or for the process group of the starter does not reach the holder.
2. It installs a handler for `SIGCHLD` that writes one byte to a pipe.
3. It makes the shared memory of the ring buffer.
4. It makes the discovery directory and writes `pid`.
5. It makes the control socket `sock`.
6. It makes the session state directory, writes the session metadata and starts the first segment of the journal.
7. It opens the PTY and starts the command. The command is the leader of a new session, and the PTY is its controlling terminal.
8. It writes `ready`.

A pooled holder does not do step 6, and its directory of step 4 is the directory of a pooled holder: see "Pooled holder".

Then the holder waits in one `poll` call for these descriptors: the `SIGCHLD` pipe, the listening socket, the PTY and each connection.
While the command runs, `poll` has a timeout only when output bytes wait in memory for the journal: 1 second at most after the holder read them. An idle holder makes no system call and uses no CPU time.

The holder uses `rustix` for the system calls. It uses `signal-hook` for the `SIGCHLD` handler, because the end of a child process has no file descriptor that is the same on Linux and macOS.

## Pooled holder

The daemon keeps a small number of holders ready for a new session: the holder pool ([daemon.md](daemon.md#holder-pool)). A pooled holder runs the shell of the user before its session exists. A new session takes it, and the holder gets its session with the `AssignSession` request. Thus a new session opens without the start of a process.

```sh
nomux-holder --pooled --session <identifier> [--directory-fifo <path>] [--start-argument <value>]... [options] -- <shell>
```

- **The identifier.** The daemon keeps a session identifier for the pooled holder when it starts it. The holder gives the command `NOMUX_SESSION_ID` with this identifier, `NOMUX_SOCK` and its token, as each holder does. Thus the environment of the shell is correct when the session takes it, and the token stays only in memory: no request carries it. The command of a running process cannot get a new environment.
- **Nothing that looks like a session.** A pooled holder writes no session state directory, no session metadata and no journal. Its discovery directory has the name `<session identifier>.pool`, which is not a session identifier: thus `discovery::scan` does not give it, and `discovery::scan_pooled` gives it. The hello of a pooled holder has the epoch 0 (`POOLED_EPOCH`), which no session has. `JournalRead` gives no bytes.
- **The daemon owns it.** The standard input of a pooled holder is a pipe from the daemon. The daemon writes no byte. When the pipe closes, because the daemon ended (also after `kill -9`) or because the daemon does not need the holder, a holder that is still pooled sends `SIGHUP` to its command and ends. Thus a pooled holder never stays without its daemon. A pooled holder whose standard input is `/dev/null` ends immediately.
- **The assignment.** `AssignSession` has the window size and the working directory of the session. The holder changes the window size if it is different, makes the session state directory with the metadata, and starts the journal at the current write position with a checkpoint of the screen. Then it renames its discovery directory to the session identifier. A connection that is open continues: the socket stays the same, and a new connection finds it by the new path. After the assignment, the holder is a holder of its session, and the pipe has no function.
- **The working directory.** The command already runs, thus the holder does not change its directory: the metadata has the directory of the session. With `--directory-fifo <path>`, the holder opens the named pipe that the daemon made at the path, with mode 0600, before it starts the command, and keeps it open for reading and writing. Thus the command can open the path before or after the assignment and does not wait for a writer. At the end of the assignment, the holder writes the working directory and a zero byte to it. After the command read them, at the next output of the command, the holder closes the pipe and removes the path. It removes the path also at its end. The start file of nomux of a pooled shell reads the directory from the pipe: [daemon.md](daemon.md#holder-pool) has the rules.
- **The start arguments.** Each `--start-argument <value>` is one more argument for the command, after its arguments. The session metadata does not have them: a session that starts again gets the command without them. The daemon gives a pooled `bash` its start file with them.

| Case | Answer to `AssignSession` |
|---|---|
| The connection does not have the lease | `Error` with `LeaseRequired` |
| The holder is not pooled: it started with its session, or it has its session already | `Error` with `InvalidPayload` |
| The working directory is not an absolute path | `Error` with `InvalidPayload` |
| The command has ended | `Error` with `ChildExited` |
| The output ends in a sequence that is not complete, thus the journal cannot start with a checkpoint | `Error` with `Failed` |
| The state directory, the journal or the rename fails | `Error` with `Failed`. The holder stays pooled |

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
The journal uses the same positions. "Epoch and sequence number" shows what a position means for a client.

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
- A window has 2 columns or more (`nomux_term::Size::MIN_COLUMNS`) and 1048576 cells at most (`WindowSize::MAX_CELLS`). A wide character uses 2 columns, and the VT library cannot keep one in a screen of 1 column. `Size::new` of `nomux-term` refuses a size that the screen state cannot have, and the holder makes each size with it. Thus the holder does not start with a window outside these limits, and it answers such a `Resize` with `InvalidPayload`: the window keeps its size.
- A holder of version 0.1.0 accepts a window of 1 column and can stop then. A daemon must not send that size.
- `Resize` changes the size of the PTY and then the size of the screen state.
- The size of the last `Resize` is the correct size of the PTY. A program can write an older size after a `Resize`: when a shell with a line editor starts a prompt, it reads the window size and writes it back, and a `Resize` between the two calls is lost. Thus, at each of the next 16 reads of output after a `Resize`, the holder reads the size of the PTY and sets it again if it is different. The program gets `SIGWINCH` again.
- The holder has no timer for this comparison. A program that started a prompt writes output after it wrote the size, and the holder compares when it reads that output. An idle holder does not wake.
- The comparison has these limits. The holder does not correct a size that a program writes after the 16 reads, or a size after which the program writes no output. A program that sets a size of its own after each change keeps it after 16 corrections at most. During the 16 reads, a size that a program sets on purpose (`stty rows`) is also set back.
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

## Session state directory

The holder keeps on disk what must stay when the holder ends and when the host starts again: the output of the session and the description of its command.
Each session has one directory in the state directory of the user. The name of the directory is the session identifier.

```
<state directory>/         mode 0700
  sessions/                mode 0700
    <session identifier>/  mode 0700
      session              the session metadata, mode 0600
      restore-policy       the restore policy, mode 0600. The daemon writes it
      journal/             mode 0700
        <epoch>-<start position>.journal   one segment, mode 0600
```

| Host | State directory |
|---|---|
| Linux | `~/.local/state/nomux` |
| macOS | `~/Library/Application Support/nomux` |

The state directory is not the runtime directory. A runtime directory can be on a file system that is empty after a restart of the host, and its files have a meaning only while their holder runs.
The path comes from `HOME` only, as the path of the runtime directory does. `nomux_holder_protocol::session_state` has the paths and the format of the metadata, and `nomux_holder_protocol::journal` has the format of the journal and its reader. The daemon reads the directory with these two modules, also when no holder runs, and it gives the holder the directory with `--state-directory`.

The holder writes the `session` file and the journal. The daemon writes one file, `restore-policy`, and the holder does not read or change it: [daemon.md](daemon.md) describes it.

If the holder cannot make the directory or the first segment, it does not start. After the start, an error of the disk does not stop the session: see "When a write fails".

### Session metadata

The file `session` is plain text, and a person can read and change it. Each line is a key, and a value after one space.

```
nomux-session 1
epoch 1
started 1795651200
rows 24
columns 80
restore on-focus
profile
directory /home/user/project
command /bin/sh
argument -c
argument echo hello
status running
```

| Key | Value |
|---|---|
| `nomux-session` | The format version: 1. It is the first line |
| `epoch` | The epoch of the holder that wrote the file |
| `started` | When that holder started: seconds after the Unix epoch |
| `rows`, `columns` | The size of the window now |
| `restore` | This version always writes `on-focus`. **The daemon does not read this key**: the restore policy of a session is in the file `restore-policy`, which the daemon owns. See the note below the table |
| `profile` | The name of the environment profile. This version writes no name |
| `directory` | The working directory of the command |
| `command` | The program of the command |
| `argument` | One argument of the command. There is one line for each argument, in their sequence |
| `status` | `running`, `exited <exit code>` or `signaled <signal number>` |
| `ended` | When the command ended: seconds after the Unix epoch. Only after the end |

- `directory`, `command` and `argument` can have each byte. In the file, `\\` is a backslash, `\n` is a line feed, `\r` is a carriage return and `\xHH` is the byte with the hexadecimal value `HH`.
- A reader ignores a key that it does not know. A later version adds keys, and the format version stays 1. `restore`, `profile`, `status` and each `argument` line are optional for a reader.
- **The file has no environment variable.** The environment of a command can have secrets, and they must not reach the disk. A later version stores the name of an environment profile in `profile`. The arguments of the command are in the file: they are visible to other users in the process list, thus the starter puts no secret there.
- The holder writes the file at its start, after each `Resize`, and at the end of the command. It writes a new file and renames it, thus a reader always gets a complete file.
- **The `restore` key is not the restore policy of the session.** The holder writes the file again from its memory, thus a value that a different process puts into the key does not stay, and the parser refuses each value but `on-focus`. The restore policy is a decision of the daemon, and the daemon keeps it in its own file. The key is a candidate for removal in a later format version.

### Cleanup rule

**The holder never removes the session state directory.** The directory stays when the command ends, when the holder ends and when the holder is killed. The daemon removes it when it forgets the session: after the retention time of an ended session, and when a client ends a session that has no holder. [daemon.md](daemon.md) has the rules.

Thus the daemon can tell two cases from the files and the discovery scan:

| `status` in `session` | Holder | Meaning |
|---|---|---|
| `exited` or `signaled` | Not important | The command ended. The journal is its full output |
| `running` | Live | The session runs |
| `running` | None | The holder was stopped while the command ran: for example, the host started again. The session is dormant, and the restore starts a new holder for it |

A holder that fails after step 6 of its start (for example, the command does not exist) leaves a directory with one empty segment. The daemon removes the directory of a new session whose holder did not start.

## Journal

The journal is the output of the session on disk. The ring buffer has the last 256 KiB of the output. The journal has the bytes before them, and it has checkpoints of the screen, thus a reader can make the screen of a position in the middle of the output.

The holder gives each chunk to the journal in the step that writes the chunk to the ring buffer and to the screen state. Thus the three always have the same bytes.

### Epoch and sequence number

- The **sequence number** of an output byte is its position: the number of bytes that the command wrote before it. The ring buffer and the journal count the same bytes.
- An **epoch** is one continuous numbering. One holder process gives one epoch, because a holder counts from 0 and its position never goes back.
- A session gets a new epoch exactly when a new holder starts for it. That is the only case in which the numbering cannot continue: the restore after a restart of the host, or a new holder after the first one was killed. The position 0 of the new epoch is the first byte that the new command wrote.
- A pooled holder starts the journal at its assignment. The first segment then starts at the write position of that moment, with a checkpoint at that position: the output before it is in the checkpoint as a screen.
- The first epoch of a session is 1. The epoch of a new holder is 1 more than the largest epoch in the `session` file and in the names of the segment files. The holder reads the two, because a person can remove each.
- The epoch is in the hello (since version 1.3) and in the `session` file. The daemon gives it to the clients as the `epoch` field of a terminal frame, and it gives the position as the `seq` field.

If the daemon removes the session state directory and then starts a holder with the same session identifier, the epoch is 1 again. The daemon removes the directory only when it forgets the session.

### Segments and records

The journal is a sequence of segment files. A file name is the epoch as 8 hexadecimal digits, `-`, the position of the first byte of the segment as 16 hexadecimal digits, and `.journal`. Thus the names in the order of text are in the order of the output, and a reader finds the segment of a position from the names.

All integers are little-endian. A segment starts with a header of 32 bytes:

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | Magic: `NMXJRNL\0` |
| 8 | 4 | Format version: 1 |
| 12 | 4 | Header length: where the first record starts. 32 in this version |
| 16 | 4 | Epoch |
| 20 | 4 | Reserved, zero |
| 24 | 8 | Start position |

Records follow the header to the end of the file. Each record is a header of 32 bytes and a payload:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | Kind: 1 output, 2 checkpoint |
| 1 | 1 | Codec of the payload: 0 stored (the bytes as they are), 1 one LZ4 block |
| 2 | 2 | Rows of the window. Zero in an output record |
| 4 | 2 | Columns of the window. Zero in an output record |
| 6 | 2 | Reserved, zero |
| 8 | 8 | Position |
| 16 | 4 | Stored length: the bytes of the payload in the file |
| 20 | 4 | Raw length: the bytes of the payload after decompression |
| 24 | 4 | CRC-32 (the polynomial of zlib) of the payload as it is in the file |
| 28 | 4 | CRC-32 of the bytes 0 to 27 of this header |

| Record | Position | Payload | Largest raw length |
|---|---|---|---|
| Output | The position of the first byte of the payload | Output bytes of the session | 131072 |
| Checkpoint | The position of the next output byte in the file | The redraw sequence of the screen | 8 MiB (`redraw::MAX_LENGTH`) |

- The output records of a segment follow each other without a gap. The first one starts at the start position of the segment.
- The holder writes a payload as an LZ4 block only if the block is shorter than the payload. Thus the stored length is never larger than the raw length.
- Each payload is one block that needs no other block. A reader can start at each record.
- A format version is in each file. A reader refuses a file with a version that it does not know. A change of the format increases the version. The codec field permits a second codec in a later version.

### Checkpoints

A checkpoint is the pair that the `Redraw` request gives, on disk: a position and the redraw sequence of the screen at that position, with the size of the window. The rule of "Redraw sequence and its position" applies: the sequence has the effect of each byte before the position, and of no byte from the position on, and the position is never in a sequence that is not complete.

Thus "the screen at position `p`" is always:

1. Find the last checkpoint whose position is not after `p` (`JournalReader::checkpoint_before`).
2. Give its redraw sequence to a new terminal that has the size of the checkpoint.
3. Give that terminal the output bytes from the position of the checkpoint to `p` (`JournalReader::read`).

A checkpoint is in the file between the output before its position and the output after it. The holder gets this order in this way: a record of output ends at the start of the escape sequence or the UTF-8 character that the output ends in (`Terminal::incomplete_tail`), and the bytes of that sequence wait in memory for the next record. Thus the end of the output in the files is a position that the screen corresponds to, and the holder can write a checkpoint there.

The holder writes a checkpoint:

| When | Position |
|---|---|
| At the start | 0. It has the size of the window at the start |
| After 1 MiB of output since the last checkpoint | The end of the record that completes the 1 MiB |
| After a `Resize` | The position at which the size changed. It is written before the next output byte, thus many changes of the size without output give one checkpoint |
| For a `Checkpoint` request | The end of the output: the holder first writes each byte that waits. The daemon asks for it before it ends the command for a hibernation, thus the journal has the screen of the moment of the stop |

- **The interval is a number of bytes, not a time.** A time needs a timer, and a timer wakes an idle holder. The holder examines the interval only when it writes a record.
- **Why 1 MiB.** A reader that starts at a checkpoint decompresses and parses 1 MiB at most. This took 3 ms to 12 ms for colored text, and 100 ms for the slowest megabyte of a colored `git log -p` (see "Measurements"). The redraw sequence of a window of 80 x 24 cells is 0.1 KiB to 4 KiB: less than 0.5 % of the output between two checkpoints. To make it takes 0.05 ms.
- **A very large screen.** The output between two checkpoints is also 16 times the stored size of the last checkpoint or more. Thus the checkpoints are 6 % of the journal at most. A screen whose redraw sequence is longer than 8 MiB gets no checkpoint.
- **The index** is the file names and the record headers. A reader finds the segment of a position from the names, reads the record headers of that segment in their sequence (it does not read the payloads of the output records) and takes the last checkpoint that is not after the position. If the segment has none, it reads the segment before it. There is no index file that can disagree with the segments.
- A reader that reads through a full epoch gives each change of the window to its terminal: when a checkpoint has a different size than the checkpoint before it, the window changed at its position.

Two cases are not exact:

- A sequence that is not complete and longer than 64 KiB cannot wait in memory. The holder writes its bytes, and writes no checkpoint until the end of the output in the files is again a position that the screen corresponds to. A `Resize` during such a sequence gets its checkpoint later than the change. Programs write such sequences for images.
- After the oldest segments were removed, the first segment can start with output and have its first checkpoint later. The screen of a position before that checkpoint cannot be made: `checkpoint_before` gives none.

### When the holder writes, and what can be lost

The holder writes with one `write` call for each record, in its one loop. It has no thread for the disk.

| Event | What the holder does |
|---|---|
| 64 KiB of output wait in memory | It writes them as one output record |
| An output byte waited for 1 second | It writes the bytes that wait as one output record. This is the only timeout of `poll` while the command runs, and it exists only while bytes wait |
| The command ended | It writes each byte that waits, also a sequence that is not complete, and calls `fdatasync` one time |

The holder never calls `fsync` or `fdatasync` while the command runs. A call for each chunk stops the read of the PTY for the time of a disk write.
A `write` call copies the record to the page cache of the kernel and returns. If the file system of the state directory is slow or does not answer (for example, a network file system), the call stops the loop for that time: the holder then reads the PTY later, and the command waits when the buffer of the PTY is full.

| Failure | What is lost |
|---|---|
| The holder is killed (`SIGKILL`, a crash) | The bytes that waited in memory: the output of the last second, 64 KiB at most, and the bytes of a sequence that was not complete. The files are a prefix of the output |
| The host loses power, or the kernel stops | Also the bytes that the kernel did not write to the disk yet. On Linux the kernel writes a changed page after 30 seconds with the default settings |
| The disk damages a file | The records of that segment from the first damaged record on. The other segments stay |

A power cut can leave a segment that ends in the middle of a record or with bytes that the holder did not write. The reader finds this from the lengths and the checksums and uses the records before it. Thus the journal needs no write order and no repair step.

### Size limit

| Limit | Value |
|---|---|
| Journal of one session, all epochs | 32 MiB of files |
| One segment | A segment that has 2 MiB or more gets no more record: the next record starts a new segment |
| One output record | 64 KiB of output, and the bytes of two reads more when a sequence is not complete |

- After each record, the holder removes the oldest segments until the journal is not larger than the limit. It never removes the segment that it writes.
- The segments of an earlier epoch of the session are the oldest. A new holder counts them at its start, thus the limit applies to the session and not to one holder.
- The limits are constants in this version (`JournalLimits::DEFAULT`). A later version gets the size limit from the configuration of the daemon, as an option of the holder.
- With the measured sizes, 32 MiB hold 50 MiB to 90 MiB of output: more than 600000 lines of 80 characters.
- A window that changes its size again and again without complete output makes one checkpoint for each change that has output after it. These checkpoints can make the open segment longer than 2 MiB, and the open segment is not removed.

### When a write fails

The session is more important than its journal. If a write to a segment fails, or if the holder cannot make the next segment (for example, the disk is full), the holder writes one line to its standard error and stops the journal: it writes no more record for the rest of its life. The ring buffer, the screen state and the requests continue. `JournalRead` gives the bytes that are in the files.

### Reading

`nomux_holder_protocol::journal::JournalReader` reads a journal directory. It needs no holder, thus the daemon uses it for a session that does not run. It also works while the holder writes: the reader gets the records that are complete.

**A reader trusts no byte of a file.** The files can be cut, damaged or made by a different program, and they must not stop the holder or the daemon.

- A segment whose header does not have the magic, a known format version, or the epoch and the start position of its file name is not a segment.
- A record whose header checksum is not correct, whose kind or codec is not known, whose lengths are not possible for its kind, or whose position is not the position that follows the records before it, ends the segment. So does a payload whose checksum is not correct or whose LZ4 block does not have the raw length of the header.
- The reader allocates the two lengths of a header only after it examined them against the limits of the format. The decoder writes only into the raw length.
- The reader uses the records of a segment up to the first one that is not valid, and it continues with the next segment. `read` then gives a range that starts after the position of the request: the caller sees that bytes are missing.

## Control protocol

The control socket is a Unix stream socket. The protocol has its own version, which is independent of the product version and of the protocol between the clients and the daemon.
The current version is 1.6. "Protocol versions" shows what each version added.

The encoding is written by hand: fixed little-endian fields with a length prefix. The holder decodes 12 small messages, thus a schema compiler and its runtime give no benefit, and the binary stays small.

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
| `0x0001` | `Hello` | Protocol major version (2), minor version (2), process ID of the command (4), rows (2), columns (2), length of the session identifier (1), session identifier, epoch (4, since version 1.3) |
| `0x0002` | `Done` | Empty |
| `0x0003` | `Error` | Error code (2) |
| `0x0004` | `Status` | State (1): 0 running, 1 exited, 2 killed by a signal. 3 bytes of zero. Exit code or signal number (4) |
| `0x0005` | `Output` | Committed position of the ring buffer (8) |
| `0x0006` | `Redraw` | Position of the ring buffer (8), length of the redraw sequence (4). Since version 1.1 |
| `0x0007` | `ProgramReports` | Revision of the program reports (8), length of the encoded program reports (4). Since version 1.2 |
| `0x0008` | `ProgramReportsChanged` | Revision of the program reports (8). Since version 1.2 |
| `0x0009` | `Journal` | Position of the first byte (8), number of bytes (4). Since version 1.3 |
| `0x000A` | `SessionToken` | The 32 characters of the token of the session. Since version 1.4 |

The holder sends `Hello` when a connection opens, before the daemon sends a message. Thus the daemon knows the protocol version before its first request. The read-only descriptor of the ring buffer comes with the hello as `SCM_RIGHTS` ancillary data.

Four messages come with a descriptor: `Hello`, `Redraw`, `ProgramReports` and a `Journal` that has bytes. A descriptor arrives with the first byte of its message, thus the daemon reads the socket with `recvmsg` each time. While the daemon waits for the reply to a `Redraw`, a `ProgramReports` or a `JournalRead` request, it sends no second request of these three kinds on that connection. Thus a descriptor that arrives after the hello always belongs to the next reply of these three kinds.

A daemon reads the epoch of a holder of version 1.2 or older as 1: such a holder has no journal, and no session has a second holder with it.

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
| `0x0103` | `Resize` | Rows (2), columns (2). 1 row or more, 2 columns or more, and 1048576 cells at most | necessary | `Done` |
| `0x0104` | `Signal` | Signal (1): 1 `SIGHUP`, 2 `SIGINT`, 3 `SIGTERM`, 4 `SIGKILL` | necessary | `Done` |
| `0x0105` | `Status` | Empty | | `Status` |
| `0x0106` | `WaitOutput` | The position that the daemon already has (8) | | `Output`, possibly later |
| `0x0107` | `Redraw` | Empty. Since version 1.1 | | `Redraw` with a descriptor |
| `0x0108` | `ProgramReports` | Empty. Since version 1.2 | | `ProgramReports` with a descriptor |
| `0x0109` | `WaitProgramReports` | The revision that the daemon already has (8). Since version 1.2 | | `ProgramReportsChanged`, possibly later |
| `0x010A` | `JournalRead` | The position of the first byte (8), the largest number of bytes (4, not 0). Since version 1.3 | | `Journal`, with a descriptor if it has bytes |
| `0x010B` | `SessionToken` | Empty. Since version 1.4 | | `SessionToken` |
| `0x010C` | `AssignSession` | Rows (2), columns (2), length of the working directory (2), the bytes of the working directory. Since version 1.5 | necessary | `Done` |
| `0x010D` | `Checkpoint` | Empty. Since version 1.6 | necessary | `Done`, or `Failed` for a pooled holder, which has no journal |

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

### Journal read

`JournalRead` gives the daemon output bytes of the current epoch from a position on. The daemon uses it for a client that attaches again with a sequence number that the ring buffer has no more, and to read the scrollback in parts. The request needs no lease.

The bytes go to the daemon as a redraw sequence does: in new shared memory without a header, with the read-only descriptor in the `Journal` reply. The reply has the position of the first byte and the number of bytes. `nomux_holder_protocol::journal::read_range` copies the bytes out of the memory.

- The holder gives the bytes from the segment files, and the bytes that still wait in memory. Thus a read goes to the write position: the daemon needs no second source for the last second of output.
- A reply has 262144 bytes at most (`journal::MAX_RANGE_LENGTH`), and it can have fewer bytes than the request permits: it ends at the end of a segment. The daemon sends the next request from the position after the last byte.
- The holder reads the files for each request. It keeps no file open and no bytes of the journal in memory between two requests.

| Case | Answer |
|---|---|
| The journal has the byte at the position of the request | `Journal` with that position, 1 byte or more, and the descriptor |
| The position is older than the oldest byte of the journal: the size limit removed the segment | `Journal` with the oldest position that exists, the bytes from there, and the descriptor. The position of the reply is larger than the position of the request |
| A segment in the middle is damaged | The same: the reply starts at the next position that the journal has |
| The journal has no byte at the position or after it | `Journal` with 0 bytes and no descriptor. The position is the end of the journal |
| The largest number of bytes is 0 | `Error` with `InvalidPayload` |
| The holder cannot read a file or make the shared memory | `Error` with `Failed` |

After the journal stopped (see "When a write fails"), the end of the journal is smaller than the write position. A daemon that gets 0 bytes for a position before the write position uses the ring buffer or a new redraw.

To give a client the bytes from the sequence number `n` of the current epoch, the daemon does these steps:

1. If the ring buffer still has the position `n`, it reads the ring buffer.
2. If not, it sends `JournalRead` with `n`. If the position of the reply is `n`, it sends the bytes and repeats this step from the end of the reply, until the ring buffer has the next position.
3. If the position of the reply is larger than `n`, the bytes are gone. The daemon then sends a redraw, as for a client without a sequence number.

To read a checkpoint, and to read an earlier epoch, the daemon uses `JournalReader` on the files. The checkpoints of a session that runs are complete in the files: the holder writes a checkpoint with one `write` call.

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
| 1.3 | Field `epoch` at the end of the hello, request `JournalRead`, reply `Journal` | The epoch is 1. The holder has no journal: a client that needs bytes that the ring buffer has no more gets a redraw, and the session has no scrollback and no state on disk |
| 1.4 | The variables `NOMUX_SESSION_ID`, `NOMUX_SOCK` and `NOMUX_TOKEN` for the command, request `SessionToken`, reply `SessionToken` | It has no token of the session. The command of that holder has none of the three variables, and the daemon refuses each self-report for the session with a message that says why. The other status sources work |
| 1.5 | Option `--pooled`, request `AssignSession`, epoch 0 in the hello of a pooled holder | Nothing changes for a holder of a session. The daemon sends `AssignSession` only to a pooled holder that it started, and only if the hello has version 1.5 or later. A pooled holder of an earlier version does not exist |
| 1.6 | Option `--launcher`, request `Checkpoint` | The daemon sends `Checkpoint` only if the hello has version 1.6 or later. A hibernation of a session of an older holder writes no checkpoint: a wake shows the last checkpoint of that holder and the output after it. The daemon gives `--launcher` only to a holder that it starts, which is of its own version |

Release 0.1.0 has a holder of version 1.2. No release has a holder of version 1.0 or 1.1, thus each holder that runs has version 1.2 or later.

The types `Request`, `Reply` and `ErrorCode` of `nomux-holder-protocol` are `#[non_exhaustive]`. Thus an addition does not stop the build of code that reads them.

### Connections and the single-writer lease

- The socket has mode 0600 in a directory with mode 0700. The holder also reads the user ID of the peer (`SO_PEERCRED` on Linux, `getpeereid` on macOS) and closes a connection from a different user.
- The holder accepts 4 connections at one time. It closes a fifth connection immediately.
- The holder continues when no connection is open, and it accepts a new connection after a disconnect.
- Each connection can read: `Status`, `WaitOutput`, `Redraw`, `ProgramReports`, `WaitProgramReports`, `JournalRead` and `SessionToken` need no lease. Only the same user can connect, and that user can read the environment of the command too.
- One connection at most has the lease. Only this connection can send `Input`, `Resize` and `Signal`. Other connections get `LeaseRequired`.
- `AcquireLease` always moves the lease to the connection that asks. The connection that had the lease gets `LeaseRequired` for its next write request.
- A disconnect releases the lease.

The last request wins because a new daemon must be able to take the lease from an old daemon that does not answer. The lease prevents two writers at the same time. It is not a security boundary: only the same user can connect.

## Discovery files

Each holder has one directory in the runtime directory of the user. The name of the directory is the session identifier. A pooled holder has the name `<session identifier>.pool` until its assignment.

```
~/.nomux/run/                 runtime directory, mode 0700
  <session identifier>/       holder directory, mode 0700
    pid                       "<process ID> <start time>\n"
    sock                      control socket, mode 0600
    ready                     empty; the holder writes it last
  <session identifier>.pool/  directory of a pooled holder, with the same files
```

The path of a socket has 103 bytes at most on macOS and 107 bytes at most on Linux. The holder checks the path of `sock` before it makes the socket: a path that is too long fails with the path, its length and the limit. The daemon opens a runtime directory with a long path through a short link in `/tmp`, thus the paths of its holders fit: [daemon.md](daemon.md#length-of-the-socket-paths) has the rule.

The start time tells a holder from a later process that got the same process ID.

| Host | Source of the start time | Unit |
|---|---|---|
| Linux | Field 22 of `/proc/<pid>/stat` | Clock ticks after the system start |
| macOS | `proc_pidinfo` with `PROC_PIDTBSDINFO`: `pbi_start_tvsec` and `pbi_start_tvusec` | Microseconds after the Unix epoch |

The value is compared only for equality.

`nomux_holder_protocol::discovery::scan` reads a runtime directory and gives the state of each holder directory. `discovery::scan_pooled` gives the directories of the pooled holders with the same states:

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

1. **Start.** The holder does the steps of "Process model". If a step fails, the holder removes its directory and exits with status 1. A usage error gives status 2. A pooled holder waits for its assignment, and ends with status 0 when the pipe of its daemon closes before it.
2. **Run.** The holder moves output from the PTY to the ring buffer, and answers requests. If all processes close the PTY but the command continues, the holder stops reading the PTY and waits for the end of the command.
3. **End of the command.** `SIGCHLD` wakes the holder. It collects the exit status, reads the output that is in the PTY (256 chunks at most), closes the PTY and answers each `WaitOutput` request. The close of the PTY sends `SIGHUP` to the processes that remain in the session. The holder writes the output that waits to the journal, calls `fdatasync` for the open segment, and writes the exit status to the session metadata.
4. **Exit status.** The holder keeps answering requests. `Status` gives the exit status. `Input`, `Resize` and `Signal` give `ChildExited`.
5. **Exit.** When a connection got the exit status, or when the exit timeout passes, the holder removes its discovery directory and exits with status 0. The session state directory stays: see "Cleanup rule".

If the holder is killed, the discovery directory stays. The scan reports it as stale.

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
| Size of `nomux-holder` (glibc, dynamic) | 624 KB | < 1 MB |
| Size of `nomux-holder` (musl, static) | 722 KB | < 1 MB |
| Resident memory, idle holder with `/bin/sh` (musl, static) | 0.8 MB | < 3 MB |
| Resident memory, ring buffer full, after 8 MB of output (musl, static) | 1.2 MB | < 3 MB |
| Resident memory, ring buffer full and program reports full (musl, static) | 1.5 MB | < 3 MB |
| Resident memory, idle holder with `/bin/sh` (glibc) | 2.6 MB | < 3 MB |
| Resident memory, ring buffer full, after 8 MB of output (glibc) | 3.1 MB | < 3 MB: not met |
| Resident memory, ring buffer full and program reports full (glibc) | 3.3 MB | < 3 MB: not met |
| CPU time and context switches in 10 s, idle | 0 and 0 | 0 |
| Size of `nomux-holder` (musl, static), with the pooled holder | 737968 bytes: 4128 bytes more than before | < 1 MB |
| Resident memory, idle pooled holder with `/bin/sh` (musl, static) | 0.8 MB to 0.9 MB | < 3 MB |
| Start of the holder to its `ready` file, with `/bin/sh`: median of 50 | 1.07 ms | |
| Output of `cat` of a 128 MiB file through the holder, plain text | 82 MiB/s | |
| Output of `cat` of a 128 MiB file through the holder, colored words | 69 MiB/s | |
| Output of `cat` of a 32 MiB file through the holder, a program status report in each line | 30 MiB/s | |
| Journal on disk for 1 MiB of output, plain words | 618 KiB | |
| Journal on disk for 1 MiB of output, colored words | 464 KiB | |
| Journal on disk for 1 MiB of output, `git log -p` with colors | 366 KiB | |
| Screen of a position from the checkpoint before it, colored words | 3 ms to 12 ms | |

"Program reports full" is the largest state that a program can make: 64 program status records and 16 notifications, each with texts of the largest length. It adds approximately 0.3 MB. A session with the reports of a real tool has some records with short texts, and its memory is the memory of the rows without "program reports full". The release archives have the static musl binary.

The idle measurement starts 4 seconds after the last output: the holder writes the bytes that wait for the journal 1 second after it read them, and makes no system call after that.

The screen state adds 134 KB to the binary and approximately 0.2 MB to the resident memory of a window of 80 x 24 cells. The program reports add 22 KB to the glibc binary and 16 KB to the musl binary. The journal, its reader and the session metadata add 64 KB to the glibc binary and 70 KB to the musl binary, and 0.1 MB to 0.2 MB to the resident memory: one block of output and one record.
During a flood of output, the VT parser uses one third to one half of the CPU time of the holder, and the kernel uses the other part.

The throughput values are the best of 11 runs on a host that did other work at the same time. One run is the time from the start of the holder to its exit, with `--exit-timeout 0`. The files are lines of 4 to 14 random words of a dictionary: the plain text, and the same with an SGR sequence before and after each word. The journal files were on a `tmpfs` file system, thus the values have the cost of the compression and of the `write` calls, and not the cost of a disk.

### Cost of the journal

With the same method and at the same time, the holder before the journal gives these values:

| Measurement | Before the journal | With the journal |
|---|---|---|
| Size of `nomux-holder` (musl, static) | 652 KB | 722 KB |
| Resident memory, idle (musl, static) | 0.8 MB | 0.8 MB |
| Resident memory, ring buffer full (musl, static) | 1.0 MB | 1.2 MB |
| Output of `cat`, plain text | 96 MiB/s | 82 MiB/s |
| Output of `cat`, colored words | 99 MiB/s | 69 MiB/s |
| Output of `cat`, a program status report in each line | 38 MiB/s | 30 MiB/s |

The journal makes a flood 15 % to 30 % slower. The holder compresses and writes in its one loop. The target for a flood is 10 MB/s (see [bench/README.md](../bench/README.md)), and the one loop is 7 times faster than that. Thus the holder has no thread for the disk: a thread adds memory and a queue with a limit, and the measurement shows no need for it.

### Selection of the codec

The plan of the project named zstd for the scrollback. The measurements selected LZ4 (`lz4_flex` 0.14.2, MIT license, safe code only).

| Codec | Size of `nomux-holder` (musl, static) | Resident memory, idle | Resident memory after 8 MB of output | Output of `cat`, plain text | Output of `cat`, colored words | Disk for 16 MiB of plain words | Disk for 16 MiB of colored words |
|---|---|---|---|---|---|---|---|
| None: the holder before the journal | 652 KB | 0.8 MB | 1.0 MB | 101 MiB/s | 101 MiB/s | | |
| zstd, the C library (`zstd-safe` 8.4.0, level 1) | 1.31 MB: not met | not measured | not measured | not measured | not measured | | |
| zstd in Rust (`ruzstd` 0.9.1, level "fastest") | 889 KB | 1.4 MB | 3.06 MB: not met | 37 MiB/s | 40 MiB/s | 8.0 MB | 6.5 MB |
| LZ4 (`lz4_flex` 0.14.2) | 722 KB | 0.8 MB | 1.3 MB | 88 MiB/s | 73 MiB/s | 10.1 MB | 7.6 MB |

- The C library adds 655 KB to a static musl binary, and its build needs a C compiler for each musl target. The value is from a program that only compresses and decompresses.
- The Rust encoder of zstd has one level. Its tables stay in memory after a flood, and it compresses 75 MiB/s to 89 MiB/s: this is slower than the holder reads.
- LZ4 uses 18 % to 26 % more disk than that zstd encoder, and it is the only one of the three that keeps the holder below its three targets. The size limit of a session is a number of bytes on disk, thus the cost is a shorter history and not a larger disk use.

The values of this table are from one set of 7 runs for each codec, thus they are not exactly the values of the first table.

A ring buffer of 1 MiB gives more than 3 MB of resident memory when it is full. This is the reason for the default of 256 KiB.

## Open points for the next versions

These parts are not in the holder yet:

- **Parts of the terminal state that the redraw sequence does not make.** The scroll region, the tab stops, the saved cursor, the character sets, the hyperlinks and the changed palette colors. The VT library gives no read access to most of them.
- **Journal.** The size limit is a constant: the holder has no option for it, and the daemon has no configuration. After a write failed, the journal stops for the rest of the life of the holder: it does not try again when the disk has space. The holder has no request for a checkpoint: the daemon reads the checkpoints from the files. The journal has no checkpoint for the screen at the end of the command.
- **Restore after a restart of the host.** The `profile` key is always empty: a restore gives the command the environment of the daemon.
- **The `restore` key of the session metadata.** The holder always writes `on-focus`, and the daemon does not read the key: the restore policy is in the file `restore-policy` of the daemon. A later format version removes the key.
- **Program reports.** The holder does not see RIS, thus a full reset does not remove the program status records. The forgery guard reads only the name of the leader of the foreground process group: it ignores the reports of a tool that runs in an interpreter, behind a pipe or on a remote host. The holder does not read OSC 99, and it does not read the text of the input line from the screen.
- **Fuzz target.** The parsers of `nomux_term::osc` have no fuzz target yet.
- **Lease notification.** The connection that loses the lease learns it from the answer to its next write request, not from a message.
- **Send across threads.** `RingReader` is not `Send`. The daemon decides how it shares a reader between tasks.
- **Compatibility tests.** When a release has a holder, CI must test the new daemon code with a holder of each older version.
- **Performance tests.** `bench` has no job that measures the targets of this document on each pull request.
