# CLI

This document describes the command-line interface (`nomux`): its commands, its JSON output and its exit codes.
The code is in `crates/nomux-cli`. The CLI is a thin client: it reads the command line, connects to the daemon with `nomux-client`, sends its requests and prints the result. Only `attach` uses the terminal of the user. [daemon.md](daemon.md) describes the daemon that answers.

## Rules

- No command asks a question. A command reads only its arguments and its environment. The exception is `attach`, which gives the keyboard and the screen of the user to a session.
- Each command has the same exit codes. `attach` adds the exit status of the command of the session: see "`attach`".
- Each command accepts `--json` and `--quiet`. The two options cannot be used together.
- A command that needs the daemon starts it when no daemon answers. The CLI runs `nomuxd --detach` and waits for 5 seconds at most until the socket answers. The commands `daemon status` and `daemon stop` start no daemon.
- The text output has colour only for the state of a session, only when the standard output is a terminal, and only when the `NO_COLOR` variable is not set. The JSON output and the quiet output never have colour.

## Commands

```
nomux [--json | --quiet] <command>
```

| Command | Function |
|---|---|
| `new [--cwd <dir>] [--attach] [-- <command> [argument...]]` | Starts a new session and prints its identifier. With `--attach`, attaches to it |
| `ls` | Lists the sessions |
| `attach <id>` | Shows a session in the terminal of the user until the user detaches or the session ends |
| `detach <id>` | Detaches the clients that are attached to a session. The session continues |
| `kill <id>` | Ends a session and waits until its command has ended |
| `daemon status` | Shows if the daemon runs |
| `daemon stop` | Stops the daemon. The sessions continue |
| `version` | Prints the version. `--version` and `-V` do the same |

### `new`

- `--cwd <dir>` is the working directory of the command. The default is the current directory of the CLI. A relative path is relative to that directory.
- `--attach` attaches the terminal to the new session, as `attach` does. The command then prints the result of the attach and not the session identifier. Without a terminal, the command fails and starts no session.
- `-- <command> [argument...]` is the command of the session. The default is the shell of the user.
- `--cmd <command> [argument...]` is a second form of the same. The two forms cannot be used together.

**Each argument after `--` or `--cmd` belongs to the command of the session, also an argument that is an option of `nomux`.** Thus the options of `nomux` come before the command:

```sh
nomux new
nomux new --json -- sleep 300            # --json is an option of nomux
nomux new -- sleep 300 --json            # --json is an argument of sleep
nomux new --cwd ~/project -- vim -u NONE notes.txt
nomux new --attach -- htop
```

The session does not need the CLI or the daemon after that.

### `attach`

`attach <id>` makes the terminal of the user the terminal of the session:

1. The session gets the window size of the terminal.
2. The terminal shows the current screen of the session. A program that already drew its screen, for example an editor, is visible immediately.
3. Each key goes to the session, and the terminal shows each byte that the session writes. The terminal is in raw mode: `Ctrl-C` and `Ctrl-Z` go to the programs of the session.
4. When the window gets a different size, the session gets the new size.

The command needs a terminal on its standard input. More than one terminal can be attached to one session at the same time: each one shows the same output, and each one can type.
The session has one window size: the size of the terminal that attached or changed its size last. A terminal with a different size shows the output as the programs made it for that size.

**Keys.** `Ctrl-\` is the prefix key of `nomux`. A terminal that is not in raw mode sends `SIGQUIT` for this key, thus almost no program gives it a function.

| Keys | Function |
|---|---|
| `Ctrl-\` and then `d` | Detach. The session continues |
| `Ctrl-\` two times | Send one `Ctrl-\` to the session |
| `Ctrl-\` and then a different key | Send the two keys to the session |

`nomux attach --help` shows these keys. The keys cannot be changed in this version.

**End.** An attach ends in one of these manners. The terminal gets its earlier mode back in each of them, also when the CLI gets a signal or fails.

| Event | Message | Exit code |
|---|---|---|
| The user detaches, `nomux detach` detaches the client, or the terminal closes | `detached from session <id>` | 0 |
| The command of the session ends | `session <id> ended with exit status <n>` | `<n>`: the exit status of the command |
| A signal ends the command of the session | `session <id> ended by signal <n>` | 128 + `<n>`, as a shell gives it |
| The session ends and the daemon cannot get the exit status | `session <id> ended; its exit status is not known` | 1 |
| The daemon stops or fails | An error that says that the session continues and how to attach again | 1 |
| The CLI gets `SIGTERM`, `SIGHUP`, `SIGINT` or `SIGQUIT` | An error | 1 |
| No session has the identifier | An error | 2 |

Thus the exit code of `attach` can be a value that the table "Exit codes" does not have, and the values 1 to 5 can come from the command of the session. With `--json`, the document of a session that ended goes to the standard output, and an error goes to the standard error.

The CLI does not use the alternate screen of the terminal: after the attach, the terminal shows the last screen of the session, and the message is on the next line.

### `detach`

`detach <id>` detaches each client that is attached to the session, from a different terminal or from a script. Each of these clients ends as after the detach keys. The session continues.
The command succeeds also if no client is attached. If no session has the identifier, the exit code is 2.

A client detaches itself with the detach keys. The command has no form without an identifier yet: a later version takes the identifier from the environment of the session.

### `kill`

The command of the session gets `SIGHUP`. If it still runs after 2 seconds, it gets `SIGKILL`.
If no session has the identifier, the exit code is 2. Thus the second `kill` of the same session has the exit code 2.

### `daemon status` and `daemon stop`

`daemon status` has the exit code 0 if a daemon runs and the exit code 2 if no daemon runs.
`daemon stop` has the exit code 0 also when no daemon runs: the result is the same.

## Output

### Text

```console
$ nomux new
k3xq2abc
$ nomux ls
ID        STATE    PID
k3xq2abc  running  41873
$ nomux attach k3xq2abc
detached from session k3xq2abc
$ nomux detach k3xq2abc
detached 0 clients from session k3xq2abc
$ nomux kill k3xq2abc
session k3xq2abc ended
$ nomux ls
no sessions
$ nomux daemon status
nomuxd 0.1.0 runs: process ID 41870, 0 sessions
$ nomux daemon stop
nomuxd stopped
```

`PID` is the process ID of the command of the session. It is `-` if the daemon does not know it.

A session has one of these states:

| State | Meaning |
|---|---|
| `running` | The command of the session runs |
| `unreachable` | The holder of the session runs, but the daemon has no connection to it. The daemon tries to connect again |
| `unknown` | The daemon is newer than the CLI and sent a state that the CLI does not know |

### Quiet

With `--quiet`, a command prints only what a script needs:

| Command | Standard output |
|---|---|
| `new` | The session identifier |
| `ls` | One session identifier on each line |
| `version` | The version number |
| `new --attach`, `attach`, `detach`, `kill`, `daemon status`, `daemon stop` | Nothing. The exit code is the result |

### Errors

An error goes to the standard error as `nomux: <message>`, and the standard output stays empty. The text of a message is not stable: a script must use the exit code.
A wrong command line prints the usage message of the argument parser and has the exit code 1.

## Exit codes

| Code | Name in JSON | Meaning |
|---:|---|---|
| 0 | | Success |
| 1 | `error` | An error that has no other code, also a wrong command line |
| 2 | `not_found` | The item does not exist: no session has the identifier, or no daemon runs (`daemon status`) |
| 3 | `timeout` | An operation did not complete in the permitted time: the daemon did not answer after its start, a session was not ready, or a command did not end |
| 4 | `no_permission` | The user does not have permission, for example for the socket of the daemon |
| 5 | `version_mismatch` | The CLI and the daemon have no common protocol version. Update the older of the two |

The codes 1 to 5 are the values of `ErrorCode` in the control protocol.

`attach` and `new --attach` have one more source of the exit code: if the session ends during the attach, the exit code is the exit status of its command. "`attach`" has the table.

## JSON output

With `--json`, a command prints one JSON document on one line.

- On success, the document goes to the standard output.
- On an error, the standard output stays empty and the document goes to the standard error. It has the field `error`.
- Each document has the field `schema_version`. The current value is `1`.

**Schema version rule.** The value increases when a field is removed, gets a different name or gets a different meaning. A new field does not increase the value, thus a reader must ignore the fields that it does not know.

### `new`

```json
{"schema_version":1,"session":{"id":"k3xq2abc","state":"running","pid":41873}}
```

| Field | Type | Meaning |
|---|---|---|
| `session.id` | string | The session identifier |
| `session.state` | string | `running`, `unreachable` or `unknown` |
| `session.pid` | number or `null` | The process ID of the command. `null` if the daemon does not know it |

### `ls`

```json
{"schema_version":1,"sessions":[{"id":"k3xq2abc","state":"running","pid":41873}]}
```

`sessions` is an array of the object that `new` gives, in the sequence of the identifiers. It is empty if there is no session.

### `attach` and `new --attach`

```json
{"schema_version":1,"attach":{"id":"k3xq2abc","end":"detached"}}
{"schema_version":1,"attach":{"id":"k3xq2abc","end":"exited","exit_code":0}}
{"schema_version":1,"attach":{"id":"k3xq2abc","end":"signaled","signal":9}}
```

The command prints the document after the attach ended.

| Field | Type | Meaning |
|---|---|---|
| `attach.id` | string | The session identifier |
| `attach.end` | string | `detached`: the client detached and the session continues. `exited`: the command ended with an exit code. `signaled`: a signal ended the command. `lost`: the session ended and its exit status is not known. `unknown`: the daemon is newer than the CLI and sent a reason that the CLI does not know |
| `attach.exit_code` | number | The exit code of the command. Only if `end` is `exited` |
| `attach.signal` | number | The signal number. Only if `end` is `signaled` |

### `detach`

```json
{"schema_version":1,"detached":{"id":"k3xq2abc","clients":2}}
```

| Field | Type | Meaning |
|---|---|---|
| `detached.id` | string | The session identifier |
| `detached.clients` | number | The number of clients that were attached to the session |

### `kill`

```json
{"schema_version":1,"killed":"k3xq2abc"}
```

`killed` is the identifier of the session that ended.

### `daemon status`

```json
{"schema_version":1,"daemon":{"running":true,"version":"0.1.0","pid":41870,"sessions":1}}
{"schema_version":1,"daemon":{"running":false}}
```

| Field | Type | Meaning |
|---|---|---|
| `daemon.running` | boolean | `true` if a daemon runs. If it is `false`, the document has no other field of `daemon` and the exit code is 2 |
| `daemon.version` | string | The product version of the daemon |
| `daemon.pid` | number | The process ID of the daemon |
| `daemon.sessions` | number | The number of sessions |

### `daemon stop`

```json
{"schema_version":1,"stopped":true}
```

`stopped` is `true` if a daemon ran and the command stopped it. It is `false` if no daemon ran.

### `version`

```json
{"schema_version":1,"version":"0.1.0"}
```

### Error

```json
{"schema_version":1,"error":{"code":2,"kind":"not_found","message":"no session has the identifier k3xq2abc"}}
```

| Field | Type | Meaning |
|---|---|---|
| `error.code` | number | The exit code of the command |
| `error.kind` | string | The name of the exit code from the table "Exit codes" |
| `error.message` | string | A message in English for a person. The text is not stable |

A wrong command line is not a JSON document: the argument parser writes its usage message as text.

## Tests

- `crates/nomux-cli/src/output/json.rs` has the full text of each document. A changed field fails these tests.
- `crates/nomux-cli/src/arguments.rs` has the rule for the arguments after `--` and `--cmd`. `crates/nomux-cli/src/attach` has the detach keys and the mode of the terminal, also after a panic.
- `tests/` has the end-to-end tests that run the real `nomux`, `nomuxd` and `nomux-holder` and examine the exit codes. `tests/attach.rs` runs `nomux attach` in a pseudoterminal and examines the screen.

## Open points for the next versions

- A setting for the detach keys, and `detach` without an identifier in a session.
- The scrollback: an attach shows the screen of the session and not the lines that left it.
- The other commands and the `--host` option of the command reference.
- The context from `NOMUX_*` variables.
