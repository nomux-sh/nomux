# CLI

This document describes the command-line interface (`nomux`): its commands, its JSON output and its exit codes.
The code is in `crates/nomux-cli`. The CLI is a thin client: it reads the command line, connects to the daemon with `nomux-client`, sends its requests and prints the result. Only `attach` uses the terminal of the user. [daemon.md](daemon.md) describes the daemon that answers, and [remote.md](remote.md) describes how a command reaches the daemon of a remote host.

## Rules

- No command asks a question. A command reads only its arguments and its environment. The exception is `attach`, which gives the keyboard and the screen of the user to a session.
- Each command has the same exit codes. `attach` adds the exit status of the command of the session: see "`attach`".
- Each command accepts `--json`, `--quiet` and `--host <name>`. `--json` and `--quiet` cannot be used together.
- Without `--host`, a command uses the daemon of the local host. With `--host <name>`, it uses the daemon of the remote host `<name>` of `nomux host ls`, through SSH: see "`--host`".
- A command that needs the daemon starts it when no daemon answers. The CLI runs `nomuxd --detach` and waits for 5 seconds at most until the socket answers. The commands `daemon status` and `daemon stop` start no daemon.
- The text output has colour only for the state of a session, only when the standard output is a terminal, and only when the `NO_COLOR` variable is not set. The JSON output and the quiet output never have colour.

## Commands

```
nomux [--json | --quiet] [--host <name>] <command>
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
| `host add <ssh-target> [--name <name>]` | Connects to a host through SSH, installs nomux there, starts the daemon and saves the host |
| `host ls` | Lists the saved hosts |
| `host rm <name>` | Removes a saved host. Nothing changes on the host |
| `host doctor [<name>]` | Checks the connection, the platform, the binaries, the permissions and the daemon of a host |
| `version` | Prints the version. `--version` and `-V` do the same |

### `--host`

`--host <name>` makes a command use the daemon of a remote host. `<name>` is a host of the global configuration file, which `nomux host add` writes.

```sh
nomux --host vps new --attach
nomux --host vps ls --json
nomux --host vps attach k3xq2abc
nomux --host vps daemon status
```

- Each command first connects to the host and makes sure that the host has the nomux binaries of the version of the CLI. If they are absent, the command installs them, as `host add` does, and writes one line about it to the standard error.
- A name that the configuration file does not have gives exit code 2.
- `new --cwd <dir>` with `--host` needs an absolute path on the host. Without `--cwd`, the command starts in the home directory of the user on the host.
- `--host` cannot be used with a `host` command, which takes the name as an argument. `version` prints the version of the CLI.
- An SSH error has exit code 1. If the host accepts no key of the user, the exit code is 4. [remote.md](remote.md) has the messages for a host key that is not known or that changed.

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
| The connection to a remote host ends | The attach connects again and continues with the output that it does not have. The title of the terminal window says `nomux: connection lost, connecting again` until then, and the keys of that time are discarded. After approximately 48 seconds without a connection: an error that says how to attach again | 1 after the error |
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

### `host add`

`host add <ssh-target> [--name <name>]` does these steps and saves the host only if all of them succeed:

1. It connects to `<ssh-target>` with the SSH client of nomux: `[user@]host`, `ssh://[user@]host[:port]`, or a `Host` name of `~/.ssh/config`.
2. It finds the operating system and the processor of the host and, if the host does not have them, installs the three binaries of the version of the CLI in `~/.nomux/bin/<version>` of the host.
3. It starts the daemon on the host, if no daemon runs, and asks it for its version.
4. It writes `[hosts.<name>]` to the global configuration file.

- The default name is the host part of the destination: `deploy@vps.example.com` gives `vps.example.com`. A name has only the characters `a` to `z`, `A` to `Z`, `0` to `9`, `.`, `_` and `-`.
- The command can be run again for the same name and the same destination: it then installs nothing that the host has. For a name that exists with a different destination, the command fails.
- The command asks no question. A host key that is not known is an error with the fingerprint and the `ssh` command that adds the key.

[remote.md](remote.md) has the steps of the installation, the variables `NOMUX_RELEASE_DOWNLOAD_URL` and `NOMUX_BOOTSTRAP_BINARIES`, the configuration file and how to remove nomux from a host.

### `host ls` and `host rm`

`host ls` reads only the configuration file. `host rm <name>` removes the host from the file, and it has the exit code 2 if the file does not have the name. The two commands do not connect to a host.

### `host doctor`

`host doctor <name>` checks one host, and `host doctor` checks each host of the configuration file. The command changes nothing on a host: it installs nothing and starts no daemon.

| Check | Passes if |
|---|---|
| `connection` | The SSH connection, the host key and the authentication are correct |
| `platform` | nomux has binaries for the operating system and the processor of the host |
| `binaries` | The host has the binaries of the version of the CLI |
| `permissions` | `~/.nomux`, `~/.nomux/bin`, `~/.nomux/run` and the socket belong to the user, other users cannot write to the first two, and other users have no access to the last two |
| `daemon` | A daemon answers, or no daemon runs. It fails if the SSH server refuses connections to Unix sockets |
| `versions` | The CLI and the daemon have a common protocol version. Only if a daemon answers |

A check that cannot run after a failure is not in the output: without a connection, the output has only `connection`.
The exit code is 0 if each check of each host passed, and 1 if a check failed.

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
$ nomux host add deploy@vps.example.com --name vps
nomux: downloading https://github.com/nomux-sh/nomux/releases/download/v0.1.0/nomux-0.1.0-x86_64-unknown-linux-musl.tar.gz
nomux: installing nomux 0.1.0 for x86_64-unknown-linux-musl on the host (5234 KiB)
host vps added: Linux x86_64, nomux in ~/.nomux/bin/0.1.0 (installed), nomuxd 0.1.0 runs: process ID 2210, 0 sessions
$ nomux host ls
NAME  SSH
vps   deploy@vps.example.com
$ nomux host doctor vps
vps
  ok    connection   deploy@vps.example.com port 22, host key ssh-ed25519 SHA256:8bLUG8wQ0mmtzh1V3IO9Yb1pOd2k9y1DqcHBnBtnOC0
  ok    platform     Linux x86_64: x86_64-unknown-linux-musl
  ok    binaries     nomux 0.1.0 is in ~/.nomux/bin/0.1.0
  ok    permissions  private to the user: ~/.nomux drwx------, ~/.nomux/bin drwx------, ~/.nomux/run drwx------, ~/.nomux/run/nomuxd.sock srw-------
  ok    daemon       nomuxd 0.1.0 runs: process ID 2210, 0 sessions
  ok    versions     the client and the daemon have the version 0.1.0
$ nomux host rm vps
host vps removed
```

A check that failed has `FAIL` and the reason:

```console
$ nomux host doctor vps
vps
  ok    connection   deploy@vps.example.com port 22, host key ssh-ed25519 SHA256:8bLUG8wQ0mmtzh1V3IO9Yb1pOd2k9y1DqcHBnBtnOC0
  ok    platform     Linux x86_64: x86_64-unknown-linux-musl
  ok    binaries     nomux 0.1.0 is in ~/.nomux/bin/0.1.0
  FAIL  permissions  ~/.nomux/run has the mode drwxr-xr-x: other users can use it. Run `chmod go-rwx ~/.nomux/run` on the host
  ok    daemon       nomuxd 0.1.0 runs: process ID 2210, 0 sessions
  ok    versions     the client and the daemon have the version 0.1.0
```

The lines that start with `nomux:` are on the standard error. Only the text output has these progress lines.

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
| `host ls` | One host name on each line |
| `version` | The version number |
| `new --attach`, `attach`, `detach`, `kill`, `daemon status`, `daemon stop`, `host add`, `host rm`, `host doctor` | Nothing. The exit code is the result |

### Errors

An error goes to the standard error as `nomux: <message>`, and the standard output stays empty. The text of a message is not stable: a script must use the exit code.
A wrong command line prints the usage message of the argument parser and has the exit code 1.

## Exit codes

| Code | Name in JSON | Meaning |
|---:|---|---|
| 0 | | Success |
| 1 | `error` | An error that has no other code, also a wrong command line, an SSH connection that cannot be made, a host key that is not trusted, and a check of `host doctor` that failed |
| 2 | `not_found` | The item does not exist: no session has the identifier, no host has the name, or no daemon runs (`daemon status`) |
| 3 | `timeout` | An operation did not complete in the permitted time: the daemon did not answer after its start, a session was not ready, or a command did not end |
| 4 | `no_permission` | The user does not have permission, for example for the socket of the daemon, or a remote host accepted no key of the user |
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

### `host add`

```json
{"schema_version":1,"host":{"name":"vps","ssh":"deploy@vps.example.com","system":"Linux","machine":"x86_64","target":"x86_64-unknown-linux-musl","directory":"~/.nomux/bin/0.1.0","installed":true,"daemon":{"running":true,"version":"0.1.0","pid":2210,"sessions":0}}}
```

| Field | Type | Meaning |
|---|---|---|
| `host.name` | string | The name of the host for `--host` |
| `host.ssh` | string | The SSH destination |
| `host.system`, `host.machine` | string | The operating system and the processor of the host, as `uname -s` and `uname -m` give them |
| `host.target` | string | The release target of the binaries of the host |
| `host.directory` | string | The directory of the binaries on the host |
| `host.installed` | boolean | `true` if the command installed the binaries. `false` if the host had them |
| `host.daemon` | object | The daemon of the host: the object of `daemon status` |

### `host ls`

```json
{"schema_version":1,"hosts":[{"name":"vps","ssh":"deploy@vps.example.com"}]}
```

`hosts` is in the sequence of the names. It is empty if the configuration file has no host.

### `host rm`

```json
{"schema_version":1,"removed":"vps"}
```

### `host doctor`

```json
{"schema_version":1,"doctor":[{"host":"vps","ok":false,"checks":[{"name":"connection","ok":true,"detail":"deploy@vps.example.com port 22, host key ssh-ed25519 SHA256:..."},{"name":"permissions","ok":false,"detail":"~/.nomux/run has the mode drwxr-xr-x: other users can use it. Run `chmod go-rwx ~/.nomux/run` on the host"}]}]}
```

| Field | Type | Meaning |
|---|---|---|
| `doctor` | array | One object for each host that the command checked |
| `doctor[].host` | string | The name of the host |
| `doctor[].ok` | boolean | `true` if each check of the host passed |
| `doctor[].checks[].name` | string | `connection`, `platform`, `binaries`, `permissions`, `daemon` or `versions` |
| `doctor[].checks[].ok` | boolean | `true` if the host passed the check |
| `doctor[].checks[].detail` | string | A text in English for a person: what the check found, or the reason and what to do. The text is not stable |

The document goes to the standard output also when a check failed. The exit code is 1 then.

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
- `tests/` has the end-to-end tests that run the real `nomux`, `nomuxd` and `nomux-holder` and examine the exit codes. `tests/attach.rs` runs `nomux attach` in a pseudoterminal and examines the screen. `tests/remote_host.rs` runs the `host` commands and the commands with `--host` against an SSH server that the test starts.

## Open points for the next versions

- A setting for the detach keys, and `detach` without an identifier in a session.
- The scrollback: an attach shows the screen of the session and not the lines that left it.
- The other commands of the command reference.
- `host doctor` for the local host, and a check of the agent hooks.
- The context from `NOMUX_*` variables.
