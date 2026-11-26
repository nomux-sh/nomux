# Memory of each session

This document describes how nomux shows the memory that the processes of each session use, and the design of an optional memory limit for a session.
The first part is implemented. The second part is a design: no code of the limit exists.

The decision is [ADR-0009](https://github.com/nomux-sh/docs/blob/main/adr/0009-resource-budgets-for-disk-and-memory.md). Its third principle applies to each part of this document: nomux measures nothing when no client asks, and the idle CPU use of the daemon stays at zero.

[daemon.md](daemon.md) describes the daemon, [holder.md](holder.md) describes the holder, and [cli.md](cli.md) has the command `nomux ls --memory`.

## What a client sees

```console
$ nomux ls --memory
ID        STATE    MEMORY     PROCESSES  HOLDER     MEASURE  COMMAND
k3xq2abc  running  512.4 MiB  7          607.0 KiB  pss      claude
m7pd4xyz  running  1.2 GiB    23         598.0 KiB  pss      cargo watch -x test
p2rs6tuv  dormant  -          -          -          -        vim notes.txt
total: 1.7 GiB in 30 processes of 2 sessions, and 1.1 MiB in the holders (pss)
```

| Value | Meaning |
|---|---|
| `MEMORY` | The memory of the process tree of the command: the command and each process that descends from it |
| `PROCESSES` | The number of processes in that tree |
| `HOLDER` | The memory of the holder of the session. **The holder is not counted in `MEMORY`**: nomux shows it as a separate value |
| `MEASURE` | How the daemon measured the two values: `pss` or `rss` |

A session without a process (dormant, ended or damaged) has no values, and it is not in the total.
The values are from the moment of the command. A second command measures again.

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

**A process that left the tree is not counted.** When a process ends, the operating system gives its children a new parent: process 1, or a process that asked for this function. A program that makes itself a daemon uses this on purpose. Such a process is not a descendant of the holder, and the daemon cannot know that it came from the session. The memory limit of the second part does not have this limit: a control group keeps each process.

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

- The daemon has no task and no timer for the memory. It reads the process table only for a `Session.Memory` request.
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

The program `tests/measurements/memory.rs` measures the first rows: [tests/README.md](../tests/README.md) says how to run it. The other rows are from these commands, with `$pid` as the process ID of the daemon:

```sh
awk '{print $14 + $15}' /proc/$pid/stat     # CPU time in clock ticks
grep ctxt /proc/$pid/status
grep Threads /proc/$pid/status
```

## Tests

- `crates/nomuxd/src/session_memory` has the rules of the process tree with tables that the tests make: a tree with a second tree beside it, a process that is older than its parent, a process that changed between the two reads, and the rule for the two measures. One test reads the real process table and finds a child of the test process.
- `tests/session_memory.rs` runs the real binaries. The command of a session allocates 48 MiB and starts a child that allocates 24 MiB: `nomux ls --memory --json` must show 2 processes and 72 MiB to 88 MiB, and less than 16 MiB for the holder. A second test makes a session dormant and examines that it has no values and is not in the total.
- `tests/remote_host.rs` sends the request through SSH with `--host`.

## Design: an optional memory limit for a session

**This part is not implemented.** It is the design for the next task, which changes the holder. The facts below were examined on Fedora 44 with systemd 259 and kernel 7.1.7, where the text says "examined". Each other fact is from the documentation of the kernel or of systemd.

### Result for the user

- The global configuration file gets one key: `memory_limit` in the `[sessions]` table, a number with the unit `M` or `G`, for example `memory_limit = "4G"`. Without the key, no session has a limit. A limit for one session, as an option of `nomux new`, is an addition to `Session.Create` and can come later.
- On Linux, the processes of a session cannot use more memory than the limit together. When they try, the kernel ends each process of that session with `SIGKILL`. The session is an ended session with the signal 9. The other sessions, the holder of the session and the daemon continue.
- `nomux ls --memory` gets one more column with the limit of each session, thus the user sees if a session has a limit.

### Mechanism: a control group of version 2 for each session

The holder puts the command of its session into a control group (cgroup) of its own. A control group keeps each process that starts in it, also a process that left the process tree.

1. The daemon gives the holder the limit as a new option: `--memory-limit <bytes>`. The daemon and the holder that it starts are from the same installation, thus the holder protocol does not change.
2. The holder finds the control group that it can use: the parent of its own control group, from `/proc/self/cgroup`. "Delegation" says why that is the correct one.
3. The holder writes `+memory` to `cgroup.subtree_control` of that control group. The write changes nothing if the controller is already there.
4. The holder makes the directory `session-<session identifier>` there and writes three files:

| File | Value | Function |
|---|---|---|
| `memory.max` | The limit in bytes | The processes of the control group cannot use more memory |
| `memory.swap.max` | `0` | The processes cannot move to the swap what they have above the limit. Without this value, a session with a limit of 4 GiB can fill the swap of the host |
| `memory.oom.group` | `1` | When the limit is reached, the kernel ends each process of the control group, not only one. A command that lost one of its processes is in a state that nobody tested |

5. The holder opens `cgroup.procs` of the new control group before it starts the command. Between `fork` and `exec`, the child writes `0` to that descriptor, which moves the child into the control group. The holder already has code at that place: `take_controlling_terminal` in `child.rs`. The write is one `write` system call, which is permitted there.
6. When the command has ended, the holder removes the directory. A control group without a process can be removed with `rmdir`.
7. A holder that got `SIGKILL` cannot remove its directory. Thus the daemon, at its start, removes each `session-*` control group that has no process (`populated 0` in `cgroup.events`) and whose session has no holder.

The holder stays in the control group of the daemon. Its memory, approximately 1 MiB, is not a part of the limit, and the limit cannot end the holder: the holder must report the end of the command.

**Examined:** in a control group with `memory.max` of 64 MiB, `memory.swap.max` of 0 and `memory.oom.group` of 1, a process that wrote to 200 MiB ended with `SIGKILL` (exit status 137 in the shell). `memory.events` had `oom_group_kill 1`, the control group had no process after that, also not the second process that the command had started, and `memory.peak` was 67108864. A process in a second control group beside it continued.

### Delegation

A process can make a control group and give it a controller only in a part of the control group tree that it owns, and only if the service manager does not manage that part. systemd calls this delegation.

| Where the daemon runs | Can the holder make the control group? |
|---|---|
| In the systemd user unit of `nomux daemon install-service`, with the two new lines below | Yes |
| In a login session, because a `nomux` command started it: the control group is `session-<n>.scope` | No. The directory belongs to `root` (examined) |
| In a container or on a host without systemd | Only if the starter of the daemon gave it a control group that the user owns |
| On a host with control groups of version 1 | No. The design uses version 2 only |

The unit of `nomux daemon install-service` gets two lines:

```ini
[Service]
Delegate=yes
DelegateSubgroup=daemon
```

- `Delegate=yes` makes the user the owner of the control group of the unit, and systemd then changes nothing below it.
- `DelegateSubgroup=daemon` starts the daemon in the control group `daemon` below the control group of the unit. This is necessary because of a rule of the kernel: a control group that has a process cannot give a controller to its children. **Examined:** with a process in the control group of the unit, the write of `+memory` to `cgroup.subtree_control` fails. With `DelegateSubgroup=daemon`, the control group of the unit has no process, the write succeeds, and a `session-a` control group beside `daemon` takes a `memory.max`.
- `DelegateSubgroup` needs systemd 254 or later. With an earlier systemd, the daemon must make the `daemon` control group and move itself there at its start.
- The user manager of systemd gives its units the `memory` controller on the hosts that were examined: `cgroup.controllers` of the unit has `cpu memory pids`. On a host where the system administrator did not give the controller to the user manager, the write of step 3 fails.
- `KillMode=process` stays: a stop of the unit must not end the holders.

The holder finds its place without a setting: its own control group is `<unit>/daemon`, thus the parent is the control group of the unit. The holder accepts the parent only if the user owns it and `cgroup.subtree_control` has `memory` after step 3.

### Without delegation

A session must start also when its limit cannot be applied: a session that does not start is a larger failure than a session without a limit. But a limit that is not there must not be silent.

- The holder starts the command without a control group and reports that to the daemon. The report is an addition to the holder protocol: one more field in the hello, with a new minor version.
- The daemon writes one line with the level `warn` to its log for the first such session, with the reason and with the command `nomux daemon install-service`.
- `nomux ls --memory` shows `-` in the column of the limit for that session, and `nomux host doctor` gets a check `memory limit` that fails with the reason when the configuration has a limit and the daemon cannot apply it.

**An alternative for a daemon outside a delegated unit** is a transient scope: the holder starts the command with `systemd-run --user --scope -p MemoryMax=<limit> -p MemorySwapMax=0 -- <command>`. The user manager then makes the control group, thus the place of the daemon does not matter. **Examined:** the scope has `memory.max` of the limit, and the start of `true` in such a scope takes 4.6 ms (median of 11), against 0.5 ms without it. The design does not select it as the first mechanism: it adds a program and a D-Bus call to each start of a session, it uses almost half of the 10 ms target for a new session, and it was not examined which process ID the holder gets for the command. It is the candidate if many users run the daemon without the unit.

### What the limit does not do

- It does not make an agent smaller, and it does not stop an idle agent. That is the hibernation of ADR-0009.
- It counts memory that the measures of the first part do not count: the kernel memory of the processes and the page cache of the files that they read. Thus a session can reach its limit while `nomux ls --memory` shows less. The kernel first frees page cache before it ends a process.
- It does not apply to a holder that already runs. A holder is never replaced, thus a new limit in the configuration applies to the sessions that start after the next start of the daemon, and to a dormant session at its restore.

### macOS

macOS has no control groups.

| Function | macOS |
|---|---|
| The memory of each session (`nomux ls --memory`) | Yes, with the measure `rss` |
| A limit for the memory of a process tree | No public interface of the operating system gives it |
| `setrlimit` with `RLIMIT_AS` | A limit for the address space of one process, not for its memory in use, and not for a tree: each child has the full limit again. The header of the kernel defines `RLIMIT_RSS` as a second name of `RLIMIT_AS`. Programs with a large address space and little memory in use, for example a JavaScript runtime, fail with a small value. Reports from other projects say that macOS does not apply the limit to each kind of allocation. **Not examined here** |
| The memory limit that macOS applies to the processes of an app (`memorystatus_control`) | A private interface that needs the `root` user or an entitlement of Apple |

Thus the design is: **on macOS, nomux applies no limit.** A `memory_limit` in the configuration of a macOS host is not applied, the daemon says so in its log one time, and `nomux ls --memory` shows `-` in the column of the limit. nomux does not use `RLIMIT_AS`: a limit that ends a program for the wrong reason is worse than no limit.
What stays for a macOS host is the visible memory of this document and the hibernation of ADR-0009, which needs no function of the operating system.

A daemon that ends the processes of a session itself when their memory is above a value would need to measure continuously. That is against the third principle of ADR-0009, thus it is not in the design.

### Work for the implementation

1. `nomux-core`: the key `memory_limit` of the `[sessions]` table.
2. `nomux-holder`: the option `--memory-limit`, the control group of the session, the move of the child, the removal at the end. A new module beside `child.rs`.
3. `nomux-holder-protocol`: the field of the hello that says if the limit is applied. A minor version.
4. `nomuxd`: the option for the holder, the removal of control groups without a process at the start, the warning.
5. `nomux-cli`: the two lines of the systemd unit, the column of the limit, the check of `host doctor`.
6. `proto`: the limit of a session in `SessionMemory`.
7. Tests: a session with a limit whose command writes to more memory than the limit ends with the signal 9 while a second session continues. The test needs a delegated control group, which the test can get with `systemd-run --user -p Delegate=yes`. On a computer without it, the test writes `SKIPPED` with the reason, as the tests of remote hosts do without `sshd`.

## Open points

- The physical footprint on macOS as a third measure.
- `nomux status` with the memory: the command does not exist yet.
- A client that shows the values continuously, and the interval that it uses.
- The memory limit: all of the design above.
