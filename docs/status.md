# Agent status

This document describes how nomux knows what runs in each session: the status model, the sources of a status, the rules of the resolver and of the aggregation, and what is not a source yet.
The `nomux-status` crate has the rules. The daemon collects what the sources report and keeps the result: [daemon.md](daemon.md#status-of-the-sessions) describes that part. The `Agent` and `Events` services of the control protocol are in `spec/control.md` of the [`proto`](https://github.com/nomux-sh/proto) repository.

The principle: each status has its source. A client can always tell a report of a program from a guess of the daemon, and a guess is never shown as a fact.

## Model

Each session has one **Agent run**: what runs in the session. An Agent run can have **Child work**: subagents, teammates and background tasks, as a tree.

| Part | Meaning |
|---|---|
| State | `idle`, `working`, `done`, `needs you` or `error`. `needs you` can have a kind: `permission`, `question` or `auth`. |
| Source | Where the state comes from. `none` if no source reports the work. |
| Message, title, progress, tool | Optional parts. Each part has its own source. |
| Since | When the daemon saw the current state and kind for the first time. |
| Seen | For `done` and `error`: if the user saw the result. |
| Aggregate | The most urgent status of the item and of each Child work below it. |

A Child work has a name below its parent. A Child work that no source reports is in the tree only if a Child work below it has a status: then its own status is empty.

## Sources

The resolver merges the reports of the sources in a fixed sequence. The first source that reports an item gives its state.

| Sequence | Source | What it says | Kind |
|---:|---|---|---|
| 1 | Program status | A program reports its status in the terminal stream with the Program Status Protocol (OSC 7501). The holder parses the reports and keeps them: [holder.md](holder.md#program-reports). A record with an `id` is a Child work: the `id` is its path. | report |
| 2 | Self-report | A program in the session reports with `nomux status set`, or with the `Agent.SetStatus` request. | report |
| 3 | Session restore | The session lost its process. What stays of its status: see "Sessions without a process". | report |
| 4 | Prompt marks | The prompt marks of the shell (OSC 133): a command runs (`working`), or the input line waits (`idle`). | guess |
| 5 | Foreground program | A program that is not the command of the session has the foreground of its terminal (`working`). | guess |

Why this sequence:

- **A program knows its own work best.** OSC 7501 and the self-report come from the program that does the work. OSC 7501 comes first because it needs no process of nomux in the session, and tools such as Claude Code, Codex and Qwen Code send it.
- **The session restore is a fact, but an old one.** It says what the session had when it lost its process. A report of a new process replaces it.
- **A guess comes last.** The prompt marks say that a command runs, not what it does. The foreground program says only that a different program has the terminal. The daemon cannot see the foreground change: the operating system does not tell it. Thus the daemon reads the foreground when a client asks for the status and when a different source changes, and a change that only this source sees makes no event.

The sequence is the order of the variants of `nomux_status::Source`. A later source, for example the events of an agent tool from an extension, is a new variant in its place. The resolver needs no change for it: it uses only the sequence.

## Rules of the resolver

| Rule | Reason |
|---|---|
| The first source that reports an item gives its state and its kind. | One fixed order: the same reports give the same status. |
| A message and a progress come from the first source that has them and that reports the same state as the decisive source. | A text of a different state describes a different state. A self-report "tests passed" must not show next to a `working` state of OSC 7501. |
| A title and a tool name come from the first source that has them. | They label the work, not a state. |
| An item keeps its `since` and its `seen` while its state and its kind do not change. A new message does not change them. | `since` is the time in the state, and the oldest wait is the most important one. A program that writes progress does not start a new wait. |
| An item with a new state gets the current time and is not seen. | A new result is new for the user. |
| Work that no source reports any more is `idle` with no source, from that moment. | The daemon does not keep a status that nothing confirms. |
| A Child work below 8 levels is ignored. After the first 64 Child work items with a status, each other one is ignored. | A program cannot make the tree, the memory or a response grow without a limit. The holder keeps 64 records at most, thus a real program does not reach the limit. |
| A message has 512 bytes at most. The daemon cuts a longer message of a program at the end of a character. | The status of each session goes into one response. |
| A program status record of `working` or `blocked` that is older than the prompt that the shell shows now is ignored. | The program that wrote it ended without a last report. A record of a result (`done`, `error`) stays. |

## Aggregation and attention

The most urgent status decides the aggregate of an item, of a session and of the host.

| Priority | Status | Reason |
|---:|---|---|
| 1 | `needs you` | The work cannot continue without the user. |
| 2 | `error`, not seen | Work stopped, and the user does not know it. |
| 3 | `done`, not seen | A result waits for the user. |
| 4 | `working` | Nothing to do now. |
| 5 | `idle`, and a result that the user saw | Nothing to look at. |

- With the same priority, the oldest `since` wins: the one that waits longest.
- With the same priority and the same `since`, the first item wins: the item itself before its Child work, and the session with the smaller identifier.
- A parent aggregates its Child work: a Child work that needs the user makes its session need the user.
- An attach to a session marks each result of the session as seen: at the start of the attach and at its end, thus a result that came during the attach is seen too.

## Sessions without a process

A dormant session has no process: no work runs and no work waits for the user. To show such work as running or as waiting is false. To show nothing loses the fact that work was there.

- When a session becomes dormant, each `working` and `needs you` status of it becomes `done` with the source "session restore". Its message described the work while it ran, thus the message does not stay. A title and a tool stay.
- A result (`done`, `error`) stays with its `since` and its `seen`.
- Idle work and a guess leave nothing.
- After a restore, the status comes from the sources of the new process. The status of the old process is gone.
- A session that was dormant when the daemon started is `idle` with no source: the daemon keeps no status on disk.
- An ended session has no Agent run.

## Self-report and the token

The holder gives the command of each session three environment variables: `NOMUX_SESSION_ID`, `NOMUX_SOCK` (the socket of the daemon) and `NOMUX_TOKEN` (a random token of 128 bits for each start of a holder). The holder gives the token to the daemon on its private socket: [holder.md](holder.md#environment-of-the-command).

- `nomux status set <working|done|blocked|error|idle> ["message"]` reads the three variables and sends `Agent.SetStatus`. The daemon accepts it only with the token of the current holder of that session, thus a program cannot report for a different session. A wrong or a missing token gives exit code 4.
- The token proves that a request comes from a process of the session. It is not a defence against a process of the same user that reads the environment of the session: such a process has the same permissions as the session.
- A self-report stays until the next one. `idle` takes it back.
- No log, no journal, no metadata file and no command output has the token.
- A holder from before protocol version 1.4 makes no token. The daemon refuses each self-report for its session with a message that says why. The other sources work. The session gets a token when its command starts again.

## Events

Each change of the Agent run of a session is an event in the event log of the daemon: the event has the Agent run after the change. The removal of a session is an event too. `nomux events --follow` prints them. The log is in memory: [daemon.md](daemon.md#events) has its limits.

## Not a source yet

- The events of an agent tool: the hooks of Claude Code, the ACP `session/update` notifications and the hooks of other tools. They come later from extensions, as a source before the program status.
- The title marks of a terminal (OSC 0 and 2), the tab status of OSC 21337, and screen content.
- The self-report of a Child work. `nomux status set` reports the Agent run of the session.
- More than one Agent run in one session, and the kind of a Child work (subagent, teammate, background task).
- Input to a session that waits for the user does not clear its attention mark. The status changes when the program reports again.
