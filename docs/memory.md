# Memory of each session

This document describes three parts that work together: how nomux shows the memory that the processes of each session use, the memory limit of a session, and the hibernation of an idle agent session, which gives the memory of its agent back and starts the agent again when the user opens the session.

The decision is [ADR-0009](https://github.com/nomux-sh/docs/blob/main/adr/0009-resource-budgets-for-disk-and-memory.md). Its third principle applies to each part of this document: nomux measures nothing when nothing changes, and the idle CPU use of the daemon stays at zero.

[daemon.md](daemon.md) describes the daemon, [holder.md](holder.md) describes the holder, [status.md](status.md) describes the status of an agent, [adapters.md](adapters.md) describes the resume command that an adapter reports, and [cli.md](cli.md) has the commands `ls --memory`, `hibernate`, `wake`, `pin` and `unpin`.

## What a client sees

```console
$ nomux ls --memory
ID        STATE       MEMORY     PROCESSES  HOLDER     MEASURE  LIMIT    FREED      COMMAND
k3xq2abc  running     512.4 MiB  7          607.0 KiB  pss      4.0 GiB  -          claude
m7pd4xyz  running     1.2 GiB    23         598.0 KiB  pss      4.0 GiB  -          cargo watch -x test
p2rs6tuv  hibernated  -          -          -          -        -        487.1 MiB  claude --resume 0b1c
total: 1.7 GiB in 30 processes of 2 sessions, and 1.1 MiB in the holders (pss)
hibernated: 487.1 MiB given back by 1 session
```

| Value | Meaning |
|---|---|
| `MEMORY` | The memory of the process tree of the command: the command and each process that descends from it |
| `PROCESSES` | The number of processes in that tree |
| `HOLDER` | The memory of the holder of the session. **The holder is not counted in `MEMORY`**: nomux shows it as a separate value |
| `MEASURE` | How the daemon measured the two values: `pss` or `rss` |
| `LIMIT` | The memory limit that the operating system applies to the processes of the command. `-` if no limit applies |
| `FREED` | For a hibernated session: the memory of its process tree just before the hibernation, which the hibernation gave back |

A session without a process (dormant, hibernated, ended or damaged) has no values in the first five columns, and it is not in the total.
The values are from the moment of the command. A second command measures again.

`nomux ls` shows a hibernated session with the state `hibernated` and the reason in the note. `nomux status` shows `hibernated: <reason>` in the place of the state. The control protocol has the state `dormant` and the field `Session.hibernation`: a client that does not know the field sees a dormant session, which is correct.

## Settings

The global configuration file has the settings. A change applies after the next start of the daemon.

```toml
[sessions]
memory_limit = "4G"            # the limit of one session: K, M or G; no key: no limit
total_memory_limit = "16G"     # the limit of all sessions together; no key: no limit

[agents]
max_live = 12                  # live agent sessions; 0 turns the count trigger off
hibernate_after = "60s"        # the idle time before a session can hibernate
hibernate_on_pressure = true   # the memory pressure of the host is a trigger
stop_signals = ["HUP", "TERM", "KILL"]
stop_wait = "5s"               # the time after each signal
```

| Key | Default | Reason for the default |
|---|---|---|
| `sessions.memory_limit` | none | A limit ends a process: only the user can know a value that does not end a correct agent. ADR-0009 makes the limit optional |
| `sessions.total_memory_limit` | none | The same reason. The free memory of a host is the job of `hibernate_on_pressure` |
| `agents.max_live` | 12 | ADR-0009: a host with a few open agents keeps them all, and a host with many idle agents gives the memory of the oldest ones back |
| `agents.hibernate_after` | 60 s | ADR-0009: an agent that did not change for a minute is not in use |
| `agents.hibernate_on_pressure` | true | The trigger acts only when processes of the host wait for memory, which a user does not want |
| `agents.stop_signals` | `HUP`, `TERM`, `KILL` | `SIGHUP` is the end of a terminal, which an agent takes as the end of its session and after which it writes its conversation. The list must end with `KILL`, thus a stop always ends |
| `agents.stop_wait` | 5 s | The time that an agent needs to write its conversation to the disk |

A value that is not valid gives an error with the key in the log of the daemon, and the daemon uses each default.

## Memory limit

One module of the daemon owns the difference of the platforms: `crates/nomuxd/src/memory_limit.rs`. The daemon examines the host one time, at its start. `nomux daemon status` shows the result as `memory limit: ...`, and `nomux host doctor` shows it as the check `memory-limit`.

| Host | What `sessions.memory_limit` does |
|---|---|
| Linux with a systemd user manager, control groups of version 2 and the `memory` controller for the user manager | **The kernel applies the limit.** The holder starts the command in a transient scope of the user manager: `systemd-run --user --scope --unit=nomux-<session>-<epoch>.scope --property=MemoryMax=<limit> --property=MemorySwapMax=0 --property=OOMPolicy=kill -- <command>`. When the processes of the session reach the limit, the kernel ends each of them with `SIGKILL`. The other sessions, the holder and the daemon continue. The limit is not a threshold of the hibernation: the kernel applies it |
| Linux without them | **No limit of the operating system.** The limit is a threshold of the hibernation: an idle agent session above it hibernates. The log of the daemon has one warning with the reason at its start |
| macOS | **No limit of the operating system:** macOS has no public interface for a limit of the memory of a process tree without privileges. The limit is a threshold of the hibernation |

- **The scope, not the delegated control group of the first design.** The design of this document before the implementation was a control group of version 2 that the holder makes in the control group of a delegated systemd unit. The implementation uses a transient scope of the user manager, for three reasons: it works where the daemon runs, also in a login session, where the control group belongs to `root`; the unit needs no `Delegate=yes` and no `DelegateSubgroup`; and the holder protocol does not change. The cost is approximately 4.6 ms for each start of a session with a limit (the measurement of the design), which a session without a limit does not have.
- `systemd-run --scope` registers its own process as the scope and then starts the command in its place with `exec`. Thus the command keeps the process ID that the holder started, the controlling terminal, the working directory and the environment of the session.
- The holder starts the scope with the option `--launcher` of the holder: [holder.md](holder.md). The session metadata has the command, not `systemd-run`, thus `ls` shows the command and a restore does not add a second scope.
- `MemorySwapMax=0`: without it, a session with a limit of 4 GiB can fill the swap of the host. `OOMPolicy=kill` sets `memory.oom.group`: the kernel ends each process of the scope, not only one, because a command that lost one of its processes is in a state that nobody tested.
- When a session ends with `SIGKILL` and has a limit, the daemon asks the user manager for the result of the scope (`systemctl --user show --property=Result`). If it is `oom-kill`, the daemon writes a warning and the event `SessionMemoryLimitReached` with the limit, and removes the failed scope (`systemctl --user reset-failed`).
- `nomux ls --memory` reads the limit of each session from the scope that has the command (`memory.max`), thus a session that started before the limit was in the configuration shows `-`.
- With a limit, the holder pool is off: a pooled shell starts before its session, thus without the limit.
- `total_memory_limit` is never a limit of the operating system, only a threshold of the hibernation. A kernel limit for all sessions together ends the process that uses the most memory, which is often an agent that works.

**Examined** with the scope, on Fedora 44 with systemd 259 and kernel 7.1.7: the scope has `memory.max` of the limit and `memory.oom.group` 1, the command keeps the process ID and the environment, a command that wrote to 100 MiB with a limit of 32 MiB ended with `SIGKILL` (exit status 137), and the user manager then had the scope as `failed` with the result `oom-kill`. The end-to-end test above runs the same path through the daemon.

**Examined** for the design: in a control group with `memory.max` of 64 MiB, `memory.swap.max` of 0 and `memory.oom.group` of 1, a process that wrote to 200 MiB ended with `SIGKILL` (exit status 137 in the shell). `memory.events` had `oom_group_kill 1`, the control group had no process after that, also not the second process that the command had started, and `memory.peak` was 67108864. A process in a second control group beside it continued.

### What the limit does not do

- It does not make an agent smaller. The hibernation gives the memory of an idle agent back.
- It counts memory that the measures of this document do not count: the kernel memory of the processes and the page cache of the files that they read. Thus a session can reach its limit while `nomux ls --memory` shows less. The kernel first frees page cache before it ends a process.
- It does not apply to a holder that already runs. A holder is never replaced, thus a new limit in the configuration applies to the sessions that start after the next start of the daemon, and to a dormant session at its restore.
- `setrlimit` with `RLIMIT_AS` is not used on macOS: it limits the address space of one process, not the memory in use of a tree, and a program with a large address space and little memory in use, for example a JavaScript runtime, fails with a small value.

## Hibernation

The hibernation stops the agent of an idle agent session that can resume, and keeps the session: its output, its kept Agent run and the resume command of its adapter. The session is dormant with a hibernation. When the user opens the session, the daemon starts the resume command in a new holder: the wake. The code is in `crates/nomuxd/src/hibernation` (the rules and the triggers) and `crates/nomuxd/src/sessions/hibernate.rs` (the stop and the wake).

An **agent session** is a session whose command runs and whose adapter gave the details of its Agent run ([adapters.md](adapters.md)). Only an agent session counts as a live agent and can hibernate.

### Triggers

| Trigger | Condition | Action | Event |
|---|---|---|---|
| Count | More live agent sessions than `agents.max_live` | Hibernates the sessions that can hibernate, the one that is idle for the longest time first, until the number is the limit | `SessionHibernated` with the reason `count`, the number of live agents and the limit |
| Memory of one session | `sessions.memory_limit` is set, the operating system does not apply it, and the processes of a session that can hibernate use more | Hibernates that session | `SessionHibernated` with the reason `session-memory`, the memory of the session and the limit |
| Memory of all sessions | `sessions.total_memory_limit` is set, and the processes of all sessions use more | Hibernates the sessions that can hibernate, the one that is idle for the longest time first, until the total without them is below the limit | `SessionHibernated` with the reason `total-memory`, the total and the limit |
| Memory pressure | `agents.hibernate_on_pressure`, and the kernel reports that processes waited for memory 200 ms in a window of 2 s (Linux PSI) | Hibernates the session that is idle for the longest time, if no other trigger of the same moment hibernates a session | `SessionHibernated` with the reason `memory-pressure`, the time of the wait and the window |
| Request | `nomux hibernate <session>` (`Session.Hibernate`) | Hibernates that session. The idle time and the pin do not apply; each other condition does | `SessionHibernated` with the reason `request` |
| Wake | `nomux attach <session>`, `nomux wake <session>` (`Session.Attach`, `Session.Start`) | Starts the resume command | `SessionWoken` with the command, or `SessionWakeFailed` with the reason and the command |
| Kernel limit | The processes of a session reach the limit that the kernel applies | The kernel ends the session. The daemon does not hibernate it: a session at its limit can work | `SessionMemoryLimitReached` with the limit |

Each event also has the memory that the hibernation gave back and its measure. The log of the daemon has one line with the reason and the numbers for each hibernation and for each failed wake.

### Conditions of each hibernation

A session hibernates only if all of these are true. `crates/nomuxd/src/hibernation/policy.rs` has them as one pure function, and its tests have one case for each condition.

| Condition | Reason |
|---|---|
| The adapter gave a resume command | Without it, a stop loses the conversation. **A session without a resume command never hibernates automatically**, and `nomux hibernate` refuses it with the reason |
| The state of the Agent run and of each Child work is `idle`, `done` (seen or not seen), or `needs you` of the kind `prompt` | Nothing runs and nothing that the agent asked for waits. A `done` result stays a result: the dormant session keeps the result and its seen mark |
| The state is not `working` | Work that runs is lost |
| The state is not `needs you` of the kind `permission`, `question` or `auth`, or without a kind | A resume loses what the agent waits for: a permission request, a question of a dialog or a login. The kind `prompt` is the input prompt of the agent, where nothing waits |
| The state is not `error` | The user must see the failure in its process |
| No client shows the session | The user looks at it |
| No input came after the last change of the state | The input line can have text that the agent did not get. The condition ends with the next change of the state |
| The user did not pin it (`nomux pin`) | The exclusion of ADR-0009. Not for `nomux hibernate` |
| The session is idle for `agents.hibernate_after`: since the last change of the state, and since the last detach of a client | A user who looked at the session a moment ago can come back. Not for `nomux hibernate` |

### What nomux never does

- It never hibernates a session for a trigger without the conditions above, and it never ends a session for its memory: only the kernel does, at a limit that the user set.
- It never gives a resume command to a shell. The command is text of an extension, thus the daemon divides it into words with the quoting of a POSIX shell only (single quotes, double quotes, backslash) and starts the first word as the program. A command with an operator, an expansion, a glob character, a comment, `~`, a line break, or a quote without its end does not start: the wake fails with the reason. `crates/nomuxd/src/hibernation/resume_command.rs` has the rule; [`spec/control.md`](https://github.com/nomux-sh/proto/blob/main/spec/control.md) of the `proto` repository specifies it.
- It never changes a resume command and never adds a word to it: no option that skips the permission checks of an agent. The adapter of Claude Code already drops such a mode from the command ([adapters.md](adapters.md)).
- It never starts a hibernated session at the start of the daemon, also with the restore policy `auto`: the session gave its memory back for the user.

### Stop

1. The daemon measures the memory of the process tree of the command: the freed memory.
2. It writes the file `hibernation` in the state directory of the session: the time, the reason, the freed memory, the resume command. A daemon that starts after the stop reads it, thus the session is hibernated and not ended, also after a restart of the daemon or of the host.
3. It asks the holder for a checkpoint of the screen (`Checkpoint`, holder protocol 1.6): the journal then has the screen of the moment of the stop. A holder of an older version has no such request; the wake then shows its last checkpoint and the output after it.
4. It sends the signals of `agents.stop_signals` to the command, with `agents.stop_wait` after each one. When the command, the session leader of its terminal, ends, the kernel sends `SIGHUP` to the processes of the foreground process group of the terminal, thus the children of the agent in that group end too.
5. When the holder reports the end, the session is dormant with its hibernation. The Agent run stays as for each dormant session ([status.md](status.md#sessions-without-a-process)), with the resume command in its details.

If the stop does not complete, the daemon removes the file and the session continues as before. A kill during a stop ends the session.

### Wake

1. A `Session.Attach` or a `Session.Start` of a hibernated session takes the resume command from the file `hibernation`.
2. The daemon divides the command into words (see above) and looks for the program in `PATH`. A command that cannot start fails the wake before a holder starts.
3. It starts a new holder with the words, in the working directory of the session, with the environment of a session ([daemon.md](daemon.md#environment-of-a-session)), and with the memory limit. The session gets the next epoch.
4. It writes the time of the wake to the file and sends `SessionWoken`. `Session.hibernation.woken_at` then says that the run is the resumed run.
5. An attach answers after the new holder is ready, and the client gets the last checkpoint of the hibernated run first, then the output after it, the frame `Restored` and the screen of the resumed run ([frame.md](https://github.com/nomux-sh/proto/blob/main/spec/frame.md)).

A wake that fails leaves the session hibernated. The request fails with the reason and with the resume command for the user to run by hand in the working directory of the session; `Session.hibernation.wake_error` and the event `SessionWakeFailed` have the reason.

The command of a resumed run is the resume command. When the agent of a session that ran a shell ends after a wake, the session ends: the shell of the old run does not come back.

### No polling

One task of the daemon applies the triggers. It wakes only for these events:

- an event of the event log: a change of a status, a hibernation, a new session;
- a detach of a client, and the end of a pin;
- a pressure report of the kernel: the reactor of the runtime of the daemon waits on the PSI trigger with the other descriptors, thus the daemon keeps one thread; the kernel reports at most one time in each window of 2 s, and only while processes wait for memory;
- the time at which a session becomes idle long enough, but only if a trigger can then apply: the count is above the limit, or a memory threshold is set. One timer, at that time.

The daemon measures the memory only when a memory threshold is set and the set of the sessions that can hibernate changed, or when the kernel reports pressure. A host where nothing changes wakes the daemon no time: "Measurements" has the counts.

On macOS, the documented source of the memory pressure is a dispatch source of the type `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`. It needs code that is not safe Rust, and the repository permits such code only after an agreement in an issue. Thus macOS has no pressure trigger yet: issue [#100](https://github.com/nomux-sh/nomux/issues/100). The count and the thresholds work on macOS.

## Measures

| Measure | Where | Meaning |
|---|---|---|
| `pss` | Linux | The proportional set size. A page that more than one process uses is divided between these processes. Thus a sum of values counts each page one time |
| `rss` | Linux for a process whose `pss` the kernel does not give, and macOS | The resident set size. A page that more than one process uses is in the value of each of them. Thus a sum of values can be larger than the memory in use |

- The daemon uses `pss` for a session only if it has that value for the holder and for each process of the command. One process without it makes each value of the session `rss`. The daemon does not add values of the two kinds.
- The sessions of one host can have different measures. The total line then says `pss and rss`, and the JSON total has `"measure":null`.
- The two measures count the pages that are in memory now. They do not count pages in the swap, and they do not count the memory that the kernel uses for the processes.
- The two measures count a page of a shared memory object or of a `tmpfs` file that a process maps.

An example of the difference: two processes of one program that share 100 MiB and each have 10 MiB of their own have 120 MiB as the sum of their `pss` values and 220 MiB as the sum of their `rss` values.

## How the daemon measures

The request is `Session.Memory` of the control protocol. [`spec/control.md`](https://github.com/nomux-sh/proto/blob/main/spec/control.md) of the `proto` repository has its rules. The code is in `crates/nomuxd/src/session_memory`.

1. The daemon takes the sessions that have a holder from its list. If there is none, it reads nothing.
2. On a thread that is not the thread of the daemon, it reads the process table of the host one time: the process ID, the parent and the start time of each process.
3. For each session, it reads the identity of the holder from the `pid` file of the holder: the process ID and the start time.
4. It finds each process that descends from the holder, and it reads the memory of the holder and of each of these processes.
5. It reads the identity of each process again and keeps only the processes that did not change.

**The process tree.** The command of a session is the only child of its holder. Thus the process tree of the command is each process that descends from the holder, and the daemon needs no record of the processes of a session. A program that starts its children in a new session or a new process group, as a shell does for each job, stays in the tree: the tree uses the parent of a process, not its group.

**A process that left the tree is not counted.** When a process ends, the operating system gives its children a new parent: process 1, or a process that asked for this function. A program that makes itself a daemon uses this on purpose. Such a process is not a descendant of the holder, and the daemon cannot know that it came from the session. The memory limit does not have this limit: a control group keeps each process.

### A process ID that the operating system used again

The daemon cannot read the process table at one instant. Between two reads, a process can end, and a new process of a different program can get its process ID. Three rules prevent that such a process is counted.

| Rule | Case that it prevents |
|---|---|
| The holder counts only if the process table has a process with the process ID and the start time of the `pid` file | The holder ended, and a different process has its process ID |
| A process that started before its parent is not a child of that parent | A process of the tree ended, a new process of the tree got its process ID, and a child of the old process still names that process ID as its parent |
| After the daemon read the memory, it reads the identity of each process again. A process counts only if it has the same start time, and only if each process between it and the holder counts | A process ended after the read of the table, and a different process has its process ID when the daemon reads the memory |

A process that has the same process ID and the same start time at two moments was the same process at each moment between them. Thus the memory that the daemon read is the memory of a process of the tree.
The cost of the rules is a value that is too small for one request, while a part of a tree ends. A value is never the memory of a process of a different program.

### Linux

- The process table is `/proc`: one directory for each process, and `/proc/<pid>/stat` for the parent and the start time.
- The memory is `/proc/<pid>/smaps_rollup`: the lines `Pss:` and `Rss:`.
- The kernel refuses `smaps_rollup` for a process that the user must not examine, for example a program with the `setuid` bit that the command started. The daemon then uses the line `VmRSS:` of `/proc/<pid>/status`, which each user can read, and the session has the measure `rss`.
- A process without a line `VmRSS:`, which is a kernel thread, is not counted.
- A process whose memory the daemon cannot read is not counted, and its descendants are not counted: the daemon cannot confirm that they descend from the holder.

For `smaps_rollup`, the kernel examines each memory area of the process. The time of a request thus increases with the memory of the sessions: "Measurements" has the values.

### macOS

- The process table is `proc_listallpids` and, for each process, `proc_pidinfo` with `PROC_PIDTBSDINFO`.
- The memory is `proc_pidinfo` with `PROC_PIDTASKINFO`: the field `pti_resident_size`. macOS has no proportional set size, thus the measure is always `rss`.
- The operating system gives the memory only for a process of the same user. A process of a different user is not counted.

**The macOS code was compiled and its tests ran only on the macOS runner of CI.** It was not examined on a Mac with real sessions. The value that Activity Monitor shows as "Memory" is the physical footprint of a process (`ri_phys_footprint` of `proc_pid_rusage`), which is not the resident set size. nomux does not use it yet.

### No sampling

- The daemon has no task and no timer for the memory. It reads the process table only for a `Session.Memory` request, and for a trigger of the hibernation that needs it: see "Hibernation".
- It keeps no value between two requests.
- The work is on a blocking thread of the runtime, as the installation of an extension. The thread ends 10 seconds after the request. An idle daemon has one thread again.
- A client that shows the values continuously sends the request again at its own interval. No such client exists yet.

### Unsafe code

The two macOS calls need `unsafe`: `proc_listallpids` in `session_memory/process_list.rs` and `proc_pidinfo` in `session_memory/process_memory.rs`. Each one is one function with a `SAFETY` comment, and the Linux build has no `unsafe` code.
The parent and the start time of a process come from `ProcessIdentity::with_parent_of` of `nomux-holder-protocol`, which already read the record of a process for the `pid` file. The daemon has no second copy of that code.

## Measurements

Measured on Linux x86-64 (Fedora 44, kernel 7.1.7, 8 processors) with the release profile.

| Measurement | Result |
|---|---|
| `Session.Memory` request, 50 sessions with 150 processes of their commands, 614 processes on the host: median, slowest of 21 requests | 9.9 ms, 11.2 ms |
| The same in a second run | 9.6 ms, 13.0 ms |
| `nomux ls --memory --json` command with these sessions: median, slowest of 21 runs | 10.9 ms, 12.0 ms |
| `nomux ls --json` command with these sessions: median, slowest of 21 runs | 1.1 ms, 1.4 ms |
| `Session.Memory` request, 200 sessions with 600 processes of their commands, 1214 processes on the host: median, slowest | 28.1 ms, 32.8 ms |
| `nomux ls --memory --json` command, 50 sessions whose commands each use 32 MiB (1.5 GiB): median, slowest of 21 runs | 17.2 ms, 23.7 ms |
| CPU time of the daemon in 42 s after two memory requests, 2 sessions | 0 clock ticks |
| Context switches of the daemon in the same 42 s | 0 |
| Threads of the daemon: before a request, immediately after a request, 12 s after a request | 1, 2, 1 |
| Size of `nomuxd` (glibc, dynamic) with the memory request | 1771 KiB (1813264 bytes) |

- The time has two parts: the read of the process table, which increases with the number of processes on the host, and the read of `smaps_rollup`, which increases with the number of processes of the sessions and with their memory.
- The daemon answers the other clients during a request: the work is not on its thread.
- The values of one session agree with the process tree: for a session with a shell and two `sleep` processes, `nomux ls --memory` showed 598.0 KiB in 3 processes, and the sum of the `Pss:` lines of the 3 processes that `pstree` shows below the holder was 597 kB a moment later.

### Hibernation

Measured on the same computer with the release profile, with 3 agent sessions of the test adapter whose agents each hold 200 MiB, by the program `tests/measurements/hibernation.rs` in a scope of its own (`systemd-run --user --scope`). The computer did other work at the same time (load average from 5 to 30 on 8 processors), thus the times have its noise.

| Measurement | Result |
|---|---|
| Memory of the commands of the 3 sessions before the hibernation (`ls --memory`, pss) | 600.7 MiB |
| Memory that the hibernations gave back (`ls --memory`, `FREED`) | 600.9 MiB |
| Memory of the control group of the measurement (each process that it started: daemon, holders, agents), before and after | 614.2 MiB, 9.5 MiB |
| Available memory of the host (`MemAvailable`), before and after: other programs changed it at the same time | 4436.5 MiB, 4636.6 MiB |
| Attach of a hibernated session to the first screen of the old run, and to the first output of the resumed run: 3 wakes | 11.7 ms and 12.2 ms, 4.2 ms and 4.2 ms, 4.2 ms and 4.2 ms |
| Idle daemon with each trigger on (count, total threshold, pressure), 60 s after 75 s of rest: CPU time, context switches of each thread, threads | 0 clock ticks, 0, 1 |

- 0 context switches in 60 s: 0 wake-ups in each minute. The PSI trigger is one more descriptor of the reactor, not a thread.
- The pressure trigger was examined with a scope of `MemoryHigh=32M` and `MemorySwapMax=0` in which a program wrote to 400 MiB: an idle agent session hibernated with the reason `memory-pressure`, and the event had 200 ms in a window of 2000 ms.
- The rest before the measurement is for the two timers that exist without this work: the adapter extension stops 60 s after its last event, and the thread of a memory measurement ends 10 s after it.
- The wake time is the start of `nomux attach` (the CLI, the connection, the start of the holder, the redraw) to the screen. The agent of the test prints its line at its start, thus the resumed run is there 0 ms to 0.5 ms after the old screen; a real agent needs its own start time after that.

The program `tests/measurements/memory.rs` measures the first rows: [tests/README.md](../tests/README.md) says how to run it. The other rows are from these commands, with `$pid` as the process ID of the daemon:

```sh
awk '{print $14 + $15}' /proc/$pid/stat     # CPU time in clock ticks
grep ctxt /proc/$pid/status
grep Threads /proc/$pid/status
```

## Tests

- `crates/nomuxd/src/session_memory` has the rules of the process tree with tables that the tests make: a tree with a second tree beside it, a process that is older than its parent, a process that changed between the two reads, and the rule for the two measures. One test reads the real process table and finds a child of the test process.
- `crates/nomuxd/src/hibernation/policy.rs` has the rules of the hibernation as a table of cases: each condition has a case that blocks the hibernation, and the count, the two thresholds and the pressure have their selection.
- `crates/nomuxd/src/hibernation/resume_command.rs` has the words of a resume command and the hostile commands that do not start.
- `crates/nomux-holder/src/journal/tests.rs` has the checkpoint of the stop.
- `tests/session_memory.rs` runs the real binaries. The command of a session allocates 48 MiB and starts a child that allocates 24 MiB: `nomux ls --memory --json` must show 2 processes and 72 MiB to 88 MiB, and less than 16 MiB for the holder. A second test makes a session dormant and examines that it has no values and is not in the total. A third test sets a limit of 64 MiB: a session that allocates 200 MiB ends with the signal 9, and a second session continues. Without a systemd user manager, the test writes `SKIPPED` with the reason.
- `tests/hibernation.rs` runs the real binaries with the test adapter and an agent tool that a test writes: two agents, one live agent at most: the agent that is idle for the longest time hibernates, its process tree is gone, `ls` shows it hibernated, `capture` has its output, and an attach shows its old screen, the mark and then the resumed run, in the working directory of the session. A second test has a wake that fails, and a hibernation that no adapter permits.
- `tests/remote_host.rs` sends the memory request through SSH with `--host`, and `host doctor` has the check `memory-limit`.
## Open points

- The memory pressure trigger on macOS (`DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`): issue [#100](https://github.com/nomux-sh/nomux/issues/100).
- Real adapters must report the idle input prompt of an agent as `needs you` of the kind `prompt`. The adapter of Claude Code reports the notification `idle_prompt` as `question` now, thus a Claude Code session at rest hibernates only before that notification comes.
- A hook that a program sends before the daemon has the token of its session is refused. A resumed agent that reports at once can thus have no details until its next hook.
- A Task that is done as a candidate at once (ADR-0009): the daemon has no data model of a Task yet.
- The physical footprint on macOS as a third measure.
- A client that shows the values continuously, and the interval that it uses.
