# CLI

This document describes the command-line interface (`nomux`): its commands, its JSON output and its exit codes.
The code is in `crates/nomux-cli`. The CLI is a thin client: it reads the command line, connects to the daemon with `nomux-client`, sends its requests and prints the result. Only `attach` uses the terminal of the user. [daemon.md](daemon.md) describes the daemon that answers, and [remote.md](remote.md) describes how a command reaches the daemon of a remote host.

## Rules

- No command asks a question. A command reads only its arguments and its environment. The exception is `attach`, which gives the keyboard and the screen of the user to a session.
- Each command has the same exit codes. `attach` adds the exit status of the command of the session: see "`attach`".
- Each command accepts `--json`, `--quiet` and `--host <name>`. `--json` and `--quiet` cannot be used together.
- Without `--host`, a command uses the daemon of the local host. With `--host <name>`, it uses the daemon of the remote host `<name>` of `nomux host ls`, through SSH: see "`--host`".
- A command that needs the daemon starts it when no daemon answers. The CLI runs `nomuxd --detach` and waits for 5 seconds at most until the socket answers. The `daemon` commands start no daemon.
- The text output has colour only for the state of a session, only when the standard output is a terminal, and only when the `NO_COLOR` variable is not set. The JSON output and the quiet output never have colour.

## Commands

```
nomux [--json | --quiet] [--host <name>] <command>
```

| Command | Function |
|---|---|
| `new [--cwd <dir>] [--attach] [-- <command> [argument...]]` | Starts a new session and prints its identifier. With `--attach`, attaches to it |
| `ls [--all]` | Lists the sessions. With `--all`, also the sessions whose command ended |
| `attach <id>` | Shows a session in the terminal of the user until the user detaches or the session ends. A dormant session starts again |
| `start <id>` | Starts the command of a dormant session again |
| `capture <id> [--lines <n>]` | Prints the last output lines of a session |
| `restore-policy <id> <policy>` | Sets when a dormant session starts again: `auto`, `on-focus` or `manual` |
| `detach <id>` | Detaches the clients that are attached to a session. The session continues |
| `kill <id>` | Ends a session and waits until its command has ended. Removes a dormant session with its output |
| `daemon status` | Shows if the daemon runs |
| `daemon stop` | Stops the daemon. The sessions continue |
| `daemon install-service [--dry-run]` | Writes the file that starts the daemon at login |
| `daemon uninstall-service [--dry-run]` | Removes that file |
| `host add <ssh-target> [--name <name>]` | Connects to a host through SSH, installs nomux there, starts the daemon and saves the host |
| `host ls` | Lists the saved hosts |
| `host rm <name>` | Removes a saved host. Nothing changes on the host |
| `host doctor [<name>]` | Checks the connection, the platform, the binaries, the permissions and the daemon of a host |
| `ext ls` | Lists the installed extensions |
| `ext install <directory> [--grant <permission>]... [--grant-all]` | Installs an extension from a directory on the host, with the permissions that the options approve |
| `ext rm <id>` | Stops an extension and removes its package, its state and its data |
| `ext enable <id>` | Enables an extension, also after the daemon disabled it because of failures |
| `ext disable <id>` | Disables an extension and stops its process |
| `ext call <id> <method> [--payload <text> \| --payload @<file>]` | Calls a method of an extension and writes the payload of its result to the standard output |
| `ext validate <directory>` | Checks the `extension.toml` of a directory on this computer. The command needs no daemon |
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
- `--host` cannot be used with a `host` command, which takes the name as an argument, and with `daemon install-service` and `daemon uninstall-service`, which change a file of the computer of the CLI. `version` prints the version of the CLI.
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

### `ls`

`ls` lists the sessions that can run: the sessions whose command runs, the dormant sessions and the damaged sessions. `ls --all` (`-a`) also lists the ended sessions, which the daemon keeps for the retention time. "Output" has the states.

### Dormant sessions: `start` and `restore-policy`

A session is dormant when its holder is gone and its command did not end. After a restart of the host, each session is dormant: `ls` shows it with its command, and no process of it runs. [daemon.md](daemon.md) has the details.

- `attach <id>` on a dormant session starts its command again and attaches to it. The command starts in the working directory of the session, with the window size of the terminal. The processes of the session from before do not come back.
- `start <id>` starts the command again without an attach, with the window size that the session had. For a session whose command runs, it succeeds and changes nothing.
- `restore-policy <id> <policy>` sets when the command starts again:

| Policy | The command starts again |
|---|---|
| `auto` | When the daemon starts, for example at login. For a development server or a watcher |
| `on-focus` | At the first `attach` or `start`. This is the default |
| `manual` | Only with `start`. `attach` fails with the exit code 1 |

The default policy is `default_policy` in the `[restore]` table of the configuration file of the host. `ls --json` shows the policy of each session.
The command of an ended session does not start again: `start` and `attach` have the exit code 2 for it.

### `capture`

`capture <id>` prints the last 100 lines that the command of the session wrote. `--lines <n>` sets the number.

- The lines come from the journal of the session. Thus the command also gives lines that left the screen, lines from before a restore, and the lines of a dormant or an ended session.
- A line ends with a line feed. The output is the bytes as the command wrote them: it has the escape sequences of the command, and a terminal session ends a line with a carriage return and a line feed.
- If the session has fewer lines, the command prints all of its output.
- With `--quiet`, the output is the same. With `--json`, the output is a string: see "JSON output".

### `attach`

`attach <id>` makes the terminal of the user the terminal of the session:

1. The session gets the window size of the terminal.
2. The terminal shows the current screen of the session. A program that already drew its screen, for example an editor, is visible immediately. For a dormant session, the terminal first shows the end of the output from before, then the line `--- nomux: the session started again. The output before this line is from the command that stopped ---`, and then the screen of the new command. The old output and the line are in the scrollback of the terminal after that.
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
| No session has the identifier, or the command of the session ended before the attach | An error | 2 |
| The session is dormant and has the restore policy `manual`, or its command cannot start | An error | 1 |

Thus the exit code of `attach` can be a value that the table "Exit codes" does not have, and the values 1 to 5 can come from the command of the session. With `--json`, the document of a session that ended goes to the standard output, and an error goes to the standard error.

The CLI does not use the alternate screen of the terminal: after the attach, the terminal shows the last screen of the session, and the message is on the next line.

### `detach`

`detach <id>` detaches each client that is attached to the session, from a different terminal or from a script. Each of these clients ends as after the detach keys. The session continues.
The command succeeds also if no client is attached. If no session has the identifier, the exit code is 2.

A client detaches itself with the detach keys. The command has no form without an identifier yet: a later version takes the identifier from the environment of the session.

### `kill`

The command of the session gets `SIGHUP`. If it still runs after 2 seconds, it gets `SIGKILL`. The session is an ended session then: `ls --all` shows it with its exit status for the retention time.
If no session has the identifier, or if its command ended already, the exit code is 2. Thus the second `kill` of the same session has the exit code 2.

A dormant session and a damaged session have no command to end. `kill` removes such a session and its state: its output is gone after that.

### `daemon status` and `daemon stop`

`daemon status` has the exit code 0 if a daemon runs and the exit code 2 if no daemon runs.
`daemon stop` has the exit code 0 also when no daemon runs: the result is the same.

### `daemon install-service` and `daemon uninstall-service`

`daemon install-service` writes the file that starts the daemon at login: a systemd user unit on Linux, a launchd agent on macOS. `daemon uninstall-service` removes it.

- With `--dry-run`, a command prints the path of the file, and `install-service` also its content. Nothing changes.
- The commands write only in the configuration directories of the user: `$XDG_CONFIG_HOME/systemd/user` or `~/.config/systemd/user` on Linux, `~/Library/LaunchAgents` on macOS. They need no `sudo`.
- The commands start and stop no program. The output of `install-service` has the command that gives the file to the service manager. The daemon that runs continues after `uninstall-service`.
- `uninstall-service` has the exit code 0 also when the file does not exist.
- `nomuxd` must be in the directory of `nomux`: the file has its path.

[daemon.md](daemon.md) has the content of the two files and what a stop of the service does to the sessions.

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

### `ext install`

`ext install <directory>` installs the extension package in `<directory>`. The directory has the manifest `extension.toml`. [extensions.md](extensions.md) describes what the daemon does with the package.

An extension gets only the permissions that the user approved. The command asks no question, thus the approval is on the command line:

| Command line | Result |
|---|---|
| No `--grant` and no `--grant-all`, and the manifest asks for a permission | The command prints the permissions that the extension asks for, installs nothing and has the exit code 4 |
| `--grant <permission>`, one time or more | The command installs the extension with these permissions. The extension does not get the other permissions that it asks for |
| `--grant-all` | The command installs the extension with each permission that it asks for. A directory that starts with `${worktree}` is not approved: give a directory with `--grant fs=<directory>` |
| The manifest asks for no permission | The command installs the extension |

A permission has one of these forms. The outputs of the `ext` commands use the same forms.

| Form | Permission of the manifest |
|---|---|
| `fs=<directory>` | An item of `fs`. For an item that starts with `${worktree}`, `<directory>` is an absolute path that ends with the rest of the item |
| `process=<program>` | An item of `process` |
| `network` | `network = true` |
| `scope=<scope>` | An item of `scopes`, for example `scope=sessions:read` |

```console
$ nomux ext install ./nomux-git
nomux: the extension nomux.git 1.0.0 asks for these permissions:
  fs=${worktree}
  process=git
  scope=sessions:read
Nothing was installed. To install the extension, approve its permissions: add --grant-all, or --grant <permission> for each permission that you approve. For a directory that starts with ${worktree}, give a directory: --grant fs=<directory>
$ nomux ext install ./nomux-git --grant-all --grant fs=/home/user/project
extension nomux.git 1.0.0 installed with the permissions: fs=/home/user/project process=git scope=sessions:read
```

- A `--grant` with a permission that the manifest does not ask for fails with the exit code 1, and nothing is installed.
- A manifest that is not valid fails with the exit code 1. The message has the path of each field that is not valid.
- An extension that is already installed fails with the exit code 1: remove it first.
- With `--host`, `<directory>` is an absolute path on the remote host. On the local host, a relative path is relative to the current directory.
- The command starts no process of the extension. The first `ext call` does.

### `ext ls`, `ext enable`, `ext disable` and `ext rm`

An extension has one of these states:

| State | Meaning |
|---|---|
| `stopped` | The extension is enabled and its process does not run. The next call starts it |
| `starting` | The process started and is not ready |
| `running` | The process runs. It stops after 60 seconds without a call |
| `failed` | The process failed. After a pause, the next call starts it again. `REASON` has the failure |
| `disabled` | The daemon does not start the extension. `REASON` says why: the user disabled it, or it failed 5 times in sequence |
| `unknown` | The daemon is newer than the CLI and sent a state that the CLI does not know |

`ext enable <id>` also enables an extension that the daemon disabled after failures. `ext disable <id>` stops the process of the extension.
`ext rm <id>` stops the process and removes the package, the approved permissions and the working directory of the extension.
Each of the three commands has the exit code 2 if no extension has the identifier.

### `ext call`

`ext call <id> <method>` calls one method of the daemon part of an extension. If the process of the extension does not run, the daemon starts it. The command is for scripts and tests.

- The payload of the call and the payload of the result are bytes. The method of the extension specifies their encoding: nomux does not change them.
- `--payload <text>` sends the bytes of the text. `--payload @<file>` sends the content of a file. Without the option, the payload is empty. A text that starts with `@` must be in a file.
- The command writes the payload of the result to the standard output, with no other byte. With `--quiet`, it does the same.

```console
$ nomux ext call nomux.echo echo --payload @request.bin > response.bin
```

| Result | Exit code |
|---|---|
| The method gave a result | 0 |
| No extension has the identifier, or the extension answered that it does not have the method or the item | 2 |
| The extension did not answer in 30 seconds, or it answered with its timeout error | 3 |
| The extension answered that it does not have the permission, for example for a host service whose scope the user did not approve | 4 |
| The extension is disabled, its process cannot start or failed, or it answered with a different error | 1 |

### `ext validate`

`ext validate <directory>` reads `<directory>/extension.toml` on this computer and checks it with the validator of the extension SDK (`nomux-extension`). The command connects to no daemon and ignores `--host`.

- A valid manifest has the exit code 0. The output has the identifier, the version, the name and the permissions that the manifest asks for.
- A manifest that is not valid has the exit code 1. The output has one line for each field that is not valid, with the path of the field, for example `extension.id` or `permissions.scopes[1]`. The output goes to the standard output.
- A directory without the file has the exit code 2.

The command does not check the files of the package. `ext install` also makes sure that the package has the program of its daemon part.

## Output

### Text

```console
$ nomux new
k3xq2abc
$ nomux ls
ID        STATE    PID    COMMAND
k3xq2abc  running  41873  /bin/zsh
$ nomux attach k3xq2abc
detached from session k3xq2abc
$ nomux detach k3xq2abc
detached 0 clients from session k3xq2abc
$ nomux restore-policy k3xq2abc manual
session k3xq2abc has the restore policy manual
$ nomux capture k3xq2abc --lines 2
running 12 tests
test result: ok. 12 passed
$ nomux kill k3xq2abc
session k3xq2abc ended
$ nomux ls
no sessions
$ nomux ls --all
ID        STATE  PID  COMMAND   NOTE
k3xq2abc  ended  -    /bin/zsh  signal 1
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
$ nomux ext validate ./nomux-git
extension.toml is valid: nomux.git 1.0.0 (Git) asks for the permissions: process=git scope=sessions:read
$ nomux ext install ./nomux-git --grant-all
extension nomux.git 1.0.0 installed with the permissions: process=git scope=sessions:read
$ nomux ext ls
ID         NAME  VERSION  STATE    PERMISSIONS                      REASON
nomux.git  Git   1.0.0    stopped  process=git,scope=sessions:read
$ nomux ext disable nomux.git
extension nomux.git disabled
$ nomux ext ls
ID         NAME  VERSION  STATE     PERMISSIONS                      REASON
nomux.git  Git   1.0.0    disabled  process=git,scope=sessions:read  the user disabled the extension
$ nomux ext enable nomux.git
extension nomux.git enabled
$ nomux ext rm nomux.git
extension nomux.git removed
$ nomux ext ls
no extensions
```

A manifest that is not valid has one line for each field:

```console
$ nomux ext validate ./nomux-git
extension.toml is not valid:
  `extension.id` must have two or more parts with `.` between them, in 64 characters at most. A part starts with a lower-case letter and has only lower-case letters, digits and `-`
```

After a restart of the host, the sessions are dormant:

```console
$ nomux ls
ID        STATE    PID  COMMAND
k3xq2abc  dormant  -    /bin/zsh
m7pd4xyz  dormant  -    vim notes.txt
$ nomux start m7pd4xyz
session m7pd4xyz runs
$ nomux daemon install-service
wrote /home/user/.config/systemd/user/nomuxd.service
To start the daemon at each login, run: systemctl --user daemon-reload && systemctl --user enable --now nomuxd.service
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

`PID` is the process ID of the command of the session. It is `-` if the session has no process or the daemon does not know it. `COMMAND` is the program of the session and its arguments, with one space between them, or `-`.
The table has the column `NOTE` only if a session has a note: how the command of an ended session ended (`exit status <n>` or `signal <n>`), and why a state file of a session is damaged.

A session has one of these states:

| State | Meaning |
|---|---|
| `running` | The command of the session runs |
| `unreachable` | The holder of the session runs, but the daemon has no connection to it. The daemon tries to connect again |
| `dormant` | The session has no process: its holder is gone, and its command did not end. `attach` or `start` starts the command again |
| `ended` | The command of the session ended. The daemon keeps the session and its output for the retention time. Only `ls --all` shows it |
| `damaged` | The daemon cannot read the description of the session. The note has the reason. `kill` removes the session |
| `unknown` | The daemon is newer than the CLI and sent a state that the CLI does not know |

### Quiet

With `--quiet`, a command prints only what a script needs:

| Command | Standard output |
|---|---|
| `new` | The session identifier |
| `ls` | One session identifier on each line |
| `capture` | The output lines, as without `--quiet` |
| `host ls` | One host name on each line |
| `ext ls` | One extension identifier on each line |
| `ext call` | The payload of the result, as without `--quiet` |
| `version` | The version number |
| `new --attach`, `attach`, `start`, `restore-policy`, `detach`, `kill`, `daemon status`, `daemon stop`, `daemon install-service`, `daemon uninstall-service`, `host add`, `host rm`, `host doctor`, `ext install`, `ext rm`, `ext enable`, `ext disable`, `ext validate` | Nothing. The exit code is the result |

### Errors

An error goes to the standard error as `nomux: <message>`, and the standard output stays empty. The text of a message is not stable: a script must use the exit code.
A wrong command line prints the usage message of the argument parser and has the exit code 1.

## Exit codes

| Code | Name in JSON | Meaning |
|---:|---|---|
| 0 | | Success |
| 1 | `error` | An error that has no other code, also a wrong command line, an SSH connection that cannot be made, a host key that is not trusted, a check of `host doctor` that failed, a dormant session that cannot start, a manifest that is not valid, and an extension that failed |
| 2 | `not_found` | The item does not exist: no session has the identifier, the command of the session ended, no host has the name, no extension has the identifier, or no daemon runs (`daemon status`) |
| 3 | `timeout` | An operation did not complete in the permitted time: the daemon did not answer after its start, a session was not ready, a command did not end, or an extension did not answer |
| 4 | `no_permission` | The user does not have permission, for example for the socket of the daemon, or a remote host accepted no key of the user. Also: `ext install` without an approval of the permissions, and an extension that does not have a permission |
| 5 | `version_mismatch` | The CLI and the daemon have no common protocol version. Update the older of the two |

The codes 1 to 5 are the values of `ErrorCode` in the control protocol.

`attach` and `new --attach` have one more source of the exit code: if the session ends during the attach, the exit code is the exit status of its command. "`attach`" has the table.

## JSON output

With `--json`, a command prints one JSON document on one line.

- On success, the document goes to the standard output.
- On an error, the standard output stays empty and the document goes to the standard error. It has the field `error`.
- Each document has the field `schema_version`. The current value is `1`.

**Schema version rule.** The value increases when a field is removed, gets a different name or gets a different meaning. A new field does not increase the value, thus a reader must ignore the fields that it does not know.

### `new` and `start`

```json
{"schema_version":1,"session":{"id":"k3xq2abc","state":"running","pid":41873,"command":["/bin/zsh"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus"}}
```

| Field | Type | Meaning |
|---|---|---|
| `session.id` | string | The session identifier |
| `session.state` | string | `running`, `unreachable`, `dormant`, `ended`, `damaged` or `unknown` |
| `session.pid` | number or `null` | The process ID of the command. `null` if the session has no process or the daemon does not know it |
| `session.command` | array of strings | The program of the command and its arguments. Empty if the daemon does not know them. A byte sequence that is not UTF-8 has U+FFFD in its place |
| `session.cwd` | string or `null` | The working directory of the command |
| `session.rows`, `session.columns` | number or `null` | The size of the window of the session |
| `session.started_at` | number or `null` | When the command started in the current epoch: seconds after the Unix epoch |
| `session.epoch` | number or `null` | The epoch of the session: 1 for a new session, and 1 more after each restore |
| `session.restore_policy` | string or `null` | `auto`, `on-focus` or `manual`. `null` for a policy that the CLI does not know |
| `session.exit` | object | Only if `state` is `ended`: `ended_at` (seconds after the Unix epoch), and `exit_code` or `signal`. The object has neither of the two if the daemon does not know how the command ended |
| `session.damage` | string | Only if a state file of the session is damaged: the reason, in English for a person. The text is not stable |

A field is `null`, or `command` is empty, when the daemon does not know the value: for a damaged session, and for a session of a holder that is too old to write the description. The document never has the environment of the command.

### `ls`

```json
{"schema_version":1,"sessions":[{"id":"k3xq2abc","state":"dormant","pid":null,"command":["/bin/zsh"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus"}]}
{"schema_version":1,"sessions":[{"id":"k3xq2abc","state":"ended","pid":null,"command":["make"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus","exit":{"ended_at":1795651260,"exit_code":2}}]}
```

`sessions` is an array of the object that `new` gives, in the sequence of the identifiers. It is empty if there is no session. An ended session is in the array only with `--all`.

### `capture`

```json
{"schema_version":1,"capture":{"id":"k3xq2abc","output":"running 12 tests\r\ntest result: ok. 12 passed\r\n"}}
```

| Field | Type | Meaning |
|---|---|---|
| `capture.id` | string | The session identifier |
| `capture.output` | string | The output lines, with their escape sequences. A byte sequence that is not UTF-8 has U+FFFD in its place: the text output of the command has the bytes without a change |

### `restore-policy`

```json
{"schema_version":1,"restore_policy":{"id":"k3xq2abc","policy":"manual"}}
```

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

### `daemon install-service` and `daemon uninstall-service`

```json
{"schema_version":1,"service":{"action":"installed","path":"/home/user/.config/systemd/user/nomuxd.service","content":"[Unit]\n..."}}
{"schema_version":1,"service":{"action":"removed","path":"/home/user/.config/systemd/user/nomuxd.service"}}
```

| Field | Type | Meaning |
|---|---|---|
| `service.action` | string | `installed`, `would_install` (a dry run), `removed`, `would_remove` (a dry run) or `not_installed`: the file does not exist, thus nothing was removed |
| `service.path` | string | The path of the service file |
| `service.content` | string | The content of the file. Only for `installed` and `would_install` |

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

### `ext ls`

```json
{"schema_version":1,"extensions":[{"id":"nomux.git","name":"Git","version":"1.0.0","state":"disabled","reason":"the user disabled the extension","permissions":{"fs":[],"process":["git"],"network":false,"scopes":["sessions:read"]}}]}
```

`extensions` is in the sequence of the identifiers. It is empty if no extension is installed.

| Field | Type | Meaning |
|---|---|---|
| `id` | string | The identifier of the extension |
| `name` | string | The name of the extension for a person |
| `version` | string | The version of the extension |
| `state` | string | `stopped`, `starting`, `running`, `failed`, `disabled` or `unknown` |
| `reason` | string or `null` | For `disabled`: why the extension is disabled. For `failed`: the failure. `null` in the other states. The text is not stable |
| `permissions.fs` | array of strings | The directories that the user approved |
| `permissions.process` | array of strings | The programs that the user approved |
| `permissions.network` | boolean | `true` if the user approved network connections |
| `permissions.scopes` | array of strings | The scopes of host services that the user approved |

### `ext install`, `ext enable` and `ext disable`

```json
{"schema_version":1,"extension":{"id":"nomux.git","name":"Git","version":"1.0.0","state":"stopped","reason":null,"permissions":{"fs":[],"process":["git"],"network":false,"scopes":["sessions:read"]}}}
```

`extension` is the object of `ext ls`, after the command. An `ext install` without an approval is an error: the standard error has the error document, with the permissions in its message.

### `ext rm`

```json
{"schema_version":1,"removed_extension":"nomux.git"}
```

### `ext call`

```json
{"schema_version":1,"call":{"extension":"nomux.echo","method":"echo","payload_base64":"CgJoaQ=="}}
```

| Field | Type | Meaning |
|---|---|---|
| `call.extension` | string | The identifier of the extension |
| `call.method` | string | The name of the method |
| `call.payload_base64` | string | The payload of the result in Base64 (RFC 4648, with padding). The payload is bytes in the encoding of the method |

If the extension answered with an error, the standard error has the error document.

### `ext validate`

```json
{"schema_version":1,"manifest":{"valid":true,"id":"nomux.git","name":"Git","version":"1.0.0","permissions":{"fs":["${worktree}"],"process":["git"],"network":false,"scopes":["sessions:read"]}}}
{"schema_version":1,"manifest":{"valid":false,"problems":[{"path":"extension.id","problem":"must have two or more parts with `.` between them, in 64 characters at most. A part starts with a lower-case letter and has only lower-case letters, digits and `-`"}]}}
```

| Field | Type | Meaning |
|---|---|---|
| `manifest.valid` | boolean | `true` if the manifest is valid. If it is `false`, the exit code is 1 |
| `manifest.id`, `manifest.name`, `manifest.version` | string | The identity in the manifest. Only if `valid` is `true` |
| `manifest.permissions` | object | The permissions that the manifest asks for, with the fields of `permissions` of `ext ls`. Only if `valid` is `true` |
| `manifest.problems[].path` | string or `null` | The path of a field that is not valid. `null` if the file is not TOML |
| `manifest.problems[].problem` | string | The rule that the field does not obey, in English for a person. The text is not stable |

The document goes to the standard output also when the manifest is not valid.

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

- `crates/nomux-cli/src/output/json.rs` and `crates/nomux-cli/src/output/extension_json.rs` have the full text of each document. A changed field fails these tests.
- `crates/nomux-cli/src/arguments.rs` has the rule for the arguments after `--` and `--cmd`. `crates/nomux-cli/src/attach` has the detach keys and the mode of the terminal, also after a panic.
- `tests/` has the end-to-end tests that run the real `nomux`, `nomuxd` and `nomux-holder` and examine the exit codes. `tests/attach.rs` runs `nomux attach` in a pseudoterminal and examines the screen. `tests/remote_host.rs` runs the `host` commands and the commands with `--host` against an SSH server that the test starts. `tests/extensions.rs` runs the `ext` commands with a test extension that uses the extension SDK. `tests/restore.rs` stops each process as a restart of the host does and runs `ls`, `attach`, `start` and `restore-policy` on the dormant sessions. `tests/journal.rs` has `capture`, and `tests/daemon_service.rs` has the service file.
- `crates/nomux-cli/src/service_files` has the content of the systemd unit and of the launchd agent, also for a path that needs quotes.

## Open points for the next versions

- A setting for the detach keys, and `detach` without an identifier in a session.
- The scrollback in an attach: an attach shows the screen of the session and not the lines that left it. `capture` prints them with their escape sequences: it does not make the text of a screen from them.
- A command that removes an ended session before its retention time is over, and a command that starts the command of an ended session again.
- A restore policy for a new session as an option of `new`.
- The other commands of the command reference, also `ext update` and `ext pack`, and the installation of an extension from a registry.
- `host doctor` for the local host, and a check of the agent hooks.
- The context from `NOMUX_*` variables.
