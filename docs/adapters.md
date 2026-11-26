# Agent adapters

This document describes how to write an adapter extension: an extension that takes the hook events of an agent tool and reports the status of the agent to the daemon.
An adapter is vendor code, thus it is an extension and not a part of the core. The core supplies the hook transport, the host services for a status report, the events of an extension for the clients, and the install of the hooks of a tool.

[extensions.md](extensions.md) describes the extension host, and [status.md](status.md) the status model. The payloads of the adapter methods and host services are in `spec/adapter.md` of the [`proto`](https://github.com/nomux-sh/proto) repository.

## Path of a hook event

```
agent tool (in a session)
  hook: nomux hook <tool> <event>     reads stdin, NOMUX_SESSION_ID, NOMUX_SOCK, NOMUX_TOKEN
    |  Hooks.Event (one request, 80 ms at most)
    v
nomuxd
  checks the token of the session
  finds the adapter of the tool       no adapter: drop with a debug log
  queue: 256 events, 4 MiB            answers the hook at once
    |  method `hook` (HookEvent), in the sequence of arrival
    v
adapter extension (started at the first event, stopped when idle)
    |  host services `agent.report`, `agent.child`
    v
status registry: source "adapter"     resolver, events, `nomux status`, `agent explain`
```

- `nomux hook` never blocks and never breaks the tool: its exit code is always 0. [cli.md](cli.md#hook) has the rules.
- The token proves that a hook event comes from a process of the session: a hook event with a wrong or a missing token is refused, as a self-report is.
- The daemon answers the hook before the adapter gets the event. The answer has an `output` field for the tool: a later version fills it for an event that waits for a decision, for example a permission request. In this version the output is empty.
- One task gives the queued events to the adapters, one event at a time. The task exists only while the queue has events. A full queue drops the new event.

## Manifest

An adapter names its tools in the `[adapter]` table and asks for two scopes:

```toml
[extension]
id = "example.testtool-adapter"
name = "testtool adapter"
version = "1.0.0"
api = "^0.1.1"

[daemon]
command = "bin/adapter"

[adapter]
tools = ["testtool"]

[permissions]
scopes = ["hooks:read", "agent:report"]
```

| Scope | Gives |
|---|---|
| `hooks:read` | The hook events of the tools of the `[adapter]` table. Without this scope, the daemon gives the extension no event |
| `agent:report` | The host services `agent.report` and `agent.child` |

- A tool name is a lower-case letter, then lower-case letters, digits and `-`, in 64 characters at most.
- The daemon gives the events of a tool to an extension that is installed and enabled, names the tool and has the approved scope `hooks:read`. If more than one extension names a tool, the extension with the smallest identifier is the adapter.
- `[adapter]` needs version 0.1.1 of the extension API. An older host refuses the manifest.

## Methods of an adapter

| Method | Argument | Result |
|---|---|---|
| `hook` | `HookEvent`: the session, the tool, the event name and the standard input of the hook (256 KiB at most; `payload_omitted` if it was larger) | `HookAnswer`: empty in this version |
| `hooks.plan` | empty | `HookInstallPlan`: the settings file of the tool and the entries that its hooks need |

The extension runs its methods one at a time. A `hook` method must be short: the next event waits for it.

## Host services

| Method | Argument | What it does |
|---|---|---|
| `agent.report` | `AgentReport` | Reports the Agent run of a session: state, kind, soft, message, title, tool, and the details (model, provider session identifier, resume command) |
| `agent.child` | `ChildWorkReport` | Starts, changes or ends one Child work: an identifier and the identifier of its parent |

- The resume command is words with the quoting of a POSIX shell and without an operator or an expansion. The daemon never gives it to a shell, and starts it unchanged for the wake of a hibernated session: [memory.md](memory.md#what-nomux-never-does). An adapter that reports no resume command makes its sessions never hibernate automatically.
- The wait of an agent at its input prompt is `AGENT_STATE_NEEDS_YOU` with the kind `NEEDS_YOU_KIND_PROMPT`, not `QUESTION`: a hibernation keeps a session at its prompt and never a session that waits for an answer.
- A report stays until the next one. `AGENT_STATE_IDLE` takes the report of the Agent run back. A new process of the session removes each report of the adapter. The details stay when the session loses its process, thus a client can show the resume command of a dormant session.
- Child work: 8 levels and 64 items at most. A new item takes the place of the oldest one that ended. An end has the state `done` or `error`.
- A text is one line of 512 bytes at most without a control character.
- An adapter with the scope `agent:report` can report for each session of the host: the token proves only where a hook event comes from, not what an extension reports.

## Soft working state and the merge rule

The hooks of a tool run in parallel, and the event of one tool call can arrive after the end of the work or while the tool waits for the user. Thus an adapter marks a `working` state that it only infers from such an event as `soft`. A `working` state that the adapter knows, for example from the submit of a prompt, is firm.

The adapter source is the first source of the status. [status.md](status.md#adapter-source) has the two tables of the rule:

- In the resolver: a soft `working` state does not decide when a later source (for example the program status of OSC 7501) reports that the item needs the user. Its title and its tool still count. A firm adapter state decides before each later source.
- In the adapter source: a soft `working` report does not replace a `needs you` state of the adapter, and it does not replace `done` work whose Child work still works (work in the background).

## Hook install description

The adapter describes the change that its hooks need as data, in `hooks.plan`. nomux computes and writes the change: `nomux hooks install <tool>` prints a unified diff and writes only with `--apply`. [cli.md](cli.md#hooks-install-hooks-uninstall-and-hooks-status) has the commands.

```
HookInstallPlan
  settings_paths: [{platform: LINUX, path: "~/.testtool/settings.json"},
                   {platform: MACOS, path: "~/.testtool/settings.json"}]
  entries:        [{pointer: "/hooks/prompt", value_json: "{\"command\":\"nomux hook testtool prompt\"}"}, ...]
```

- A path starts with `~/` and has no `.` or `..` part. The client refuses a plan without a path for its platform.
- An entry names an array of the JSON settings file with a JSON pointer, and a value that the array must contain. nomux makes each object and the array on the path. A value that the array has already is not added again.
- The command of a hook is `nomux hook <tool> <event>`. nomux never wraps or replaces the command of the tool, and never adds an option that skips the permission checks of the tool.
- The uninstall restores the bytes of the file if the file did not change after the install, and removes only the entries of the plan if it changed, with each list and object that the install made and that has no other value then. The settings file is strict JSON: Claude Code 2.1 refuses a file with a comment or with a comma after the last value, thus nomux does too.

## Test adapter

`tests/support/test_adapter.rs` is the adapter of the made-up tool `testtool` that the end-to-end tests use. It maps four events (`prompt`, `tool`, `task`, `stop`) to status reports, gives the resume command `testtool --resume <session>` or the field `resume` of the event, sends the extension event `mapped` after each event, and describes the hooks for `~/.testtool/settings.json`. It is not the adapter of a real tool: the real adapters are in the [`extensions`](https://github.com/nomux-sh/extensions) repository.

## Measurements

`tests/measurements/hooks.rs` measures the wall time of `nomux hook` from the start of the process to its end, as a tool runs it, with the test adapter installed. Release build, Linux x86-64, 100 runs each:

| `nomux hook` | Median | Fastest | Slowest |
|---|---|---|---|
| daemon runs, adapter installed | 1.0 ms | 0.9 ms | 2.6 ms |
| daemon stopped | 0.9 ms | 0.8 ms | 1.4 ms |

## Not done yet

- Real adapters (they are in the `extensions` repository), approval cards and the answer of a hook with a decision of the user.
- The adapter of Claude Code reports its notification `idle_prompt` as `NEEDS_YOU_KIND_QUESTION`: it must report `NEEDS_YOU_KIND_PROMPT`, else a Claude Code session at rest does not hibernate.
- Events from the daemon to an extension.
- `hooks install` for a remote host, and settings files that are not JSON.
- A queue for each adapter: an adapter that hangs delays the events of the other adapters until its call times out.
