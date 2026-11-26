# Agent status

This document describes how nomux knows what runs in each session: the status model, the sources of a status, the rules of the resolver and of the aggregation, and what is not a source yet.
The `nomux-status` crate has the rules. The daemon collects what the sources report and keeps the result: [daemon.md](daemon.md#status-of-the-sessions) describes that part. The `Agent` and `Events` services of the control protocol are in `spec/control.md` of the [`proto`](https://github.com/nomux-sh/proto) repository.

The principle: each status has its source. A client can always tell a report of a program from a guess of the daemon, and a guess is never shown as a fact.

## Model

Each session has one **Agent run**: what runs in the session. An Agent run can have **Child work**: subagents, teammates and background tasks, as a tree.

| Part | Meaning |
|---|---|
| State | `idle`, `working`, `done`, `needs you` or `error`. `needs you` can have a kind: `permission`, `question`, `auth` or `prompt`. `prompt` is the input prompt of the agent: nothing that it asked for waits, thus a hibernation of the session loses nothing ([memory.md](memory.md#conditions-of-each-hibernation)). |
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
| 1 | Adapter | An adapter extension reports the status from the events of the agent tool, for example its hooks: [adapters.md](adapters.md). It also reports Child work with an identifier and a parent. | report |
| 2 | Program status | A program reports its status in the terminal stream with the Program Status Protocol (OSC 7501). The holder parses the reports and keeps them: [holder.md](holder.md#program-reports). A record with an `id` is a Child work: the `id` is its path. | report |
| 3 | Self-report | A program in the session reports with `nomux status set`, or with the `Agent.SetStatus` request. | report |
| 4 | Session restore | The session lost its process. What stays of its status: see "Sessions without a process". | report |
| 5 | Prompt marks | The prompt marks of the shell (OSC 133): a command runs (`working`), or the input line waits (`idle`). | guess |
| 6 | Foreground program | A program that is not the command of the session has the foreground of its terminal (`working`). | guess |

Why this sequence:

- **The events of the tool are the most detailed report.** The hooks of a tool say which tool runs, what a permission request asks and when each subagent starts and ends. The adapter comes first, with one exception for its soft working state: "Adapter source" has the rule.
- **A program knows its own work best.** OSC 7501 and the self-report come from the program that does the work. OSC 7501 comes before the self-report because it needs no process of nomux in the session, and tools such as Claude Code, Codex and Qwen Code send it.
- **The session restore is a fact, but an old one.** It says what the session had when it lost its process. A report of a new process replaces it.
- **A guess comes last.** The prompt marks say that a command runs, not what it does. The foreground program says only that a different program has the terminal. The daemon cannot see the foreground change: the operating system does not tell it. Thus the daemon reads the foreground when a client asks for the status and when a different source changes, and a change that only this source sees makes no event.

The sequence is the order of the variants of `nomux_status::Source`. A later source is a new variant in its place. The adapter source came this way: the resolver uses only the sequence, and one rule for a soft working state, which is a part of an observation and not of a source.

## Rules of the resolver

| Rule | Reason |
|---|---|
| The first source that reports an item gives its state and its kind. | One fixed order: the same reports give the same status. |
| A soft working state does not decide when a later source reports that the item needs the user. The next source decides then. | A tool event is not proof that the wait for the user ended. "Adapter source" has the table. |
| A message and a progress come from the first source that has them and that reports the same state as the decisive source. | A text of a different state describes a different state. A self-report "tests passed" must not show next to a `working` state of OSC 7501. |
| A title and a tool name come from the first source that has them. | They label the work, not a state. |
| An item keeps its `since` and its `seen` while its state and its kind do not change. A new message does not change them. | `since` is the time in the state, and the oldest wait is the most important one. A program that writes progress does not start a new wait. |
| An item with a new state gets the current time and is not seen. | A new result is new for the user. |
| Work that no source reports any more is `idle` with no source, from that moment. | The daemon does not keep a status that nothing confirms. |
| A Child work below 8 levels is ignored. After the first 64 Child work items with a status, each other one is ignored. | A program cannot make the tree, the memory or a response grow without a limit. The holder keeps 64 records at most, thus a real program does not reach the limit. |
| A message has 512 bytes at most. The daemon cuts a longer message of a program at the end of a character. | The status of each session goes into one response. |
| A program status record of `working` or `blocked` that is older than the prompt that the shell shows now is ignored. | The program that wrote it ended without a last report. A record of a result (`done`, `error`) stays. |

## Adapter source

An adapter extension reports with the host services `agent.report` and `agent.child`: [adapters.md](adapters.md). `nomux_status::AdapterSource` keeps what one adapter reported for one session. The source has the same rules as the other sources for the source labels, for `since` and `seen`, and for the aggregation.

An adapter marks a `working` state as **soft** when it only infers it from the event of one tool call: hooks run in parallel, and such an event can arrive after the end of the work or while the tool waits for the user. A firm state is a state that the adapter knows, for example the submit of a prompt.

How the adapter source and a later source (the program status, the self-report, ...) merge when the two report the same item:

| Adapter | Later source | Result | Test |
|---|---|---|---|
| firm state | any state | the state of the adapter | `a firm adapter state decides before a later state that needs the user`, `a firm adapter result decides before a later working state` |
| soft `working` | `needs you` | `needs you` of the later source; the title and the tool of the adapter stay | `a soft working state yields to a later state that needs the user`, `a_soft_working_state_that_yields_keeps_its_tool_and_not_its_message` |
| soft `working` | `idle`, `working`, `done`, `error` | `working` of the adapter | `a soft working state decides before a later result` |
| soft `working` | nothing | `working` of the adapter | `a soft working state decides without a later source` |
| nothing | any state | the state of the later source | `without an adapter report the later source decides` |

The same rule applies to each Child work (`a_soft_working_child_work_yields_to_a_later_state_that_needs_the_user`). The tests are in `crates/nomux-status/tests/adapter_rules.rs`.

What a soft `working` report does to the state that the adapter reported before for the same item:

| Before | After a soft `working` report | Test |
|---|---|---|
| `needs you` | `needs you` stays | `a wait for the user stays` |
| `done`, and a Child work below it works (work in the background) | `done` stays | `complete work with working Child work is work in the background and stays` |
| `done`, and no Child work below it works | `working` | `complete work without working Child work works again` |
| `error` | `working` | `a failure works again` |

A firm report replaces each state. `idle` takes the report of the Agent run back.

- Child work of the adapter has an identifier and the identifier of its parent. Its path in the tree is the identifiers from the Agent run down. An adapter has 64 Child work items at most in a session: a new item takes the place of the oldest one that ended and has no Child work below it. 8 levels at most.
- An ended Child work keeps its last state: `done` or `error`.
- The adapter gives the **details** of the Agent run: the model, the identifier of the session of the tool and the resume command. They are not a part of a status: clients get them in `AgentRun.details`. They stay when the session loses its process, and a new process removes them.

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
- A Group or a Project of the workspace tree aggregates the sessions below it with the same rule. A client computes it: the tree has no status. [workspace.md](workspace.md#status) describes it.
- An attach to a session marks each result of the session as seen: at the start of the attach and at its end, thus a result that came during the attach is seen too.

## Sessions without a process

A dormant session has no process: no work runs and no work waits for the user. To show such work as running or as waiting is false. To show nothing loses the fact that work was there.

- When a session becomes dormant, each `working` and `needs you` status of it becomes `done` with the source "session restore". Its message described the work while it ran, thus the message does not stay. A title and a tool stay.
- A result (`done`, `error`) stays with its `since` and its `seen`.
- Idle work and a guess leave nothing.
- After a restore, the status comes from the sources of the new process. The status of the old process is gone.
- A session that was dormant when the daemon started is `idle` with no source: the daemon keeps no status on disk.

### Hibernated sessions

A hibernated session is a dormant session: its Agent run follows the rule above, and the details stay with the resume command. The daemon also keeps the resume command in the file `hibernation` of the state directory, thus a wake after a restart of the daemon has it. After a wake, the status comes from the sources of the resumed run, and `Session.hibernation.woken_at` marks the run as resumed. The daemon uses the facts of a run for the hibernation only when its status task has a connection to the current holder: before that, the entry has the run of the process before the wake. [memory.md](memory.md#hibernation) has the rules.

### Ended sessions

When the command of a session is the agent tool itself, the session ends with the tool, and the result of the run matters most at that moment. Thus an ended session keeps its last Agent run: the **kept run**.

- When the command ends, the Agent run becomes a kept run with the rule of a dormant session: `working` and `needs you` become `done` with the source "session restore", a result stays with its `since` and its `seen`, and Child work that ran becomes `done`.
- The details stay: the identifier of the session of the tool and the resume command.
- A run with nothing that stays, and no details, is not kept: the session has no Agent run.
- The kept run is in `Agent.Tree` (`nomux status --tree`) and `Agent.Explain` (`nomux agent explain <id>`) gives it, until the session is removed: at the end of the retention time, or with its state. The removal is an `AgentRunRemoved` event.
- No source can change a kept run: the session has no process. An adapter report and a self-report for it are refused.
- Reading a kept run does not mark it seen. Only an attach marks a result seen, and an ended session has no attach. A request that marks a result seen is not in the control protocol yet.
- **Limit: 64 kept runs.** When a 65th session ends with a run, the oldest kept run is removed, with an `AgentRunRemoved` event; the session stays ended. One run has 64 Child work items at most and texts of 512 bytes at most, thus the kept runs use a few MiB at most.
- A daemon that starts keeps no run of an ended session: the daemon keeps no status on disk.

## Self-report and the token

The holder gives the command of each session three environment variables: `NOMUX_SESSION_ID`, `NOMUX_SOCK` (the socket of the daemon) and `NOMUX_TOKEN` (a random token of 128 bits for each start of a holder). The daemon makes the token before it starts the holder and keeps it in the status registry, thus it accepts a report from the first instruction of the command: [holder.md](holder.md#environment-of-the-command).

- `nomux status set <working|done|blocked|error|idle> ["message"]` reads the three variables and sends `Agent.SetStatus`. The daemon accepts it only with the token of the current holder of that session, thus a program cannot report for a different session. A wrong or a missing token gives exit code 4.
- The token proves that a request comes from a process of the session. It stops a process of one session that reports for a different session by mistake or with a guessed identifier. It does not stop a process of the same user that reads the environment of a process of the session or of its holder in `/proc`: such a process has the same permissions as the session. The daemon gives the token to the holder in its environment, not in its arguments: a process of a different user can read the arguments of a process (`/proc/<pid>/cmdline`, `ps`), but not its environment. The holder has no other channel from the daemon before its start: the standard input of a holder that is not pooled is `/dev/null`.
- A self-report stays until the next one. `idle` takes it back.
- No log, no journal, no metadata file and no command output has the token.
- A holder from before protocol version 1.4 makes no token. The daemon refuses each self-report for its session with a message that says why. The other sources work. The session gets a token when its command starts again.

## Events

Each change of the Agent run of a session is an event in the event log of the daemon: the event has the Agent run after the change. The removal of a session is an event too. `nomux events --follow` prints them. The log is in memory: [daemon.md](daemon.md#events) has its limits.

## Not a source yet

- The ACP `session/update` notifications. An extension can report them through the adapter source.
- The title marks of a terminal (OSC 0 and 2), the tab status of OSC 21337, and screen content.
- The self-report of a Child work. `nomux status set` reports the Agent run of the session.
- More than one Agent run in one session, and the kind of a Child work (subagent, teammate, background task).
- Input to a session that waits for the user does not clear its attention mark. The status changes when the program reports again.
- A request that marks a result seen without an attach. Thus the kept run of an ended session stays unseen until the session is removed.
