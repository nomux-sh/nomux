# CLI

This document describes the command-line interface (`nomux`): its commands, its JSON output and its exit codes.
The code is in `crates/nomux-cli`. The CLI is a thin client: it reads the command line, makes one connection with `nomux-client`, sends one request and prints the result. [daemon.md](daemon.md) describes the daemon that answers.

## Rules

- No command asks a question. A command reads only its arguments and its environment.
- Each command has the same exit codes.
- Each command accepts `--json` and `--quiet`. The two options cannot be used together.
- A command that needs the daemon starts it when no daemon answers. The CLI runs `nomuxd --detach` and waits for 5 seconds at most until the socket answers. The commands `daemon status` and `daemon stop` start no daemon.
- The text output has colour only for the state of a session, only when the standard output is a terminal, and only when the `NO_COLOR` variable is not set. The JSON output and the quiet output never have colour.

## Commands

```
nomux [--json | --quiet] <command>
```

| Command | Function |
|---|---|
| `new [--cwd <dir>] [--cmd <command> [argument...]]` | Starts a new session and prints its identifier |
| `ls` | Lists the sessions |
| `kill <id>` | Ends a session and waits until its command has ended |
| `daemon status` | Shows if the daemon runs |
| `daemon stop` | Stops the daemon. The sessions continue |
| `version` | Prints the version. `--version` and `-V` do the same |

### `new`

- `--cwd <dir>` is the working directory of the command. The default is the current directory of the CLI. A relative path is relative to that directory.
- `--cmd <command> [argument...]` is the command of the session. All the arguments after `--cmd` belong to the command, thus `--cmd` is the last option. The default is the shell of the user.

```sh
nomux new
nomux new --cwd ~/project --cmd vim -u NONE notes.txt
```

The session does not need the CLI or the daemon after that. To get the terminal of a session, a later version adds `attach`.

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
| `kill`, `daemon status`, `daemon stop` | Nothing. The exit code is the result |

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
- `tests/` has the end-to-end tests that run the real `nomux`, `nomuxd` and `nomux-holder` and examine the exit codes.

## Open points for the next versions

- `attach <id>` and `detach`.
- The other commands and the `--host` option of the command reference.
- The context from `NOMUX_*` variables.
