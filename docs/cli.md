# CLI

This document describes the command-line interface (`nomux`): its commands, its JSON output and its exit codes.
The code is in `crates/nomux-cli`. The CLI is a thin client: it reads the command line, connects to the daemon with `nomux-client`, sends its requests and prints the result. Only `attach` uses the terminal of the user. [daemon.md](daemon.md) describes the daemon that answers, and [remote.md](remote.md) describes how a command reaches the daemon of a remote host.

## Rules

- No command asks a question, but `kill --all` on a terminal: a command that ends many sessions never runs without a sign of the user. A command reads only its arguments and its environment. The other exception is `attach`, which gives the keyboard and the screen of the user to a session.
- Each command has the same exit codes. `attach` adds the exit status of the command of the session: see "`attach`".
- Each command accepts `--json`, `--quiet` and `--host <name>`. `--json` and `--quiet` cannot be used together.
- Without `--host`, a command uses the daemon of the local host. With `--host <name>`, it uses the daemon of the remote host `<name>` of `nomux host ls`, through SSH: see "`--host`".
- A command that needs the daemon starts it when no daemon answers. The CLI runs `nomuxd --detach` and waits for 5 seconds at most until the socket answers. The `daemon` commands start no daemon.
- The text output has colour only for the state of a session or of its status, only when the standard output is a terminal, and only when the `NO_COLOR` variable is not set. The JSON output and the quiet output never have colour.
- On a terminal, the text output fits the width of the terminal. A table shortens its columns first: the command column, then the column of the note, the message or the reason, and then a column of a path or a name. An ellipsis (`…`) marks the cut, and a shortened column keeps the width of its header. Thus the state and the message stay visible.
- If the shortened columns are not enough, a table leaves out columns in a fixed sequence: `ls` leaves out `PID` and then `PROJECT` (it never leaves out `NAME`), `ls --memory` leaves out `MEASURE`, then `PROCESSES` and then `HOLDER`, `status` leaves out `FOR` and then `COMMAND`, and `ext ls` leaves out `VERSION` and then `NAME`. A table leaves out a column only when the table does not fit with each shortened column at the width of its header. A line that is not a table, for example the total line of `ls --memory`, breaks at a space. With usual values, each table fits a terminal of 60 columns.
- The width is the display width of the text: a wide character has two columns and is not cut. When the standard output is not a terminal, and in the JSON output, nothing is shortened, left out or broken.

## Commands

```
nomux [--json | --quiet] [--host <name>] <command>
nomux
```

Bare `nomux` on a terminal attaches to the most recent session (see `attach` without a target), or starts a session and attaches to it if the host has none. Without a terminal on the standard input and the standard output, it prints the help on the standard error and has the exit code 1. `nomux --help` prints the help.

`a` is an alias of `attach`. For the habit of tmux, `attach-session`, `new-session`, `list-sessions`, `kill-session` and `rename-session` are aliases too; the help does not show them. Their options are the options of nomux, not of tmux.

| Command | Function |
|---|---|
| `new [-n <name>] [-a \| -A] [-c <dir>] [-- <command> [argument...]]` | Starts a new session and prints its identifier. With `-a`, attaches to it. With `-A`, attaches to the session that has the name, or starts it |
| `ls [--all] [--memory]` | Lists the sessions. With `--all`, also the sessions whose command ended. With `--memory`, also the memory that the processes of each session use |
| `attach [-d] [-r] [--prefix <key>]... [<target>]` | Shows a session in the terminal of the user until the user detaches or the session ends. A dormant session starts again. Alias: `a` |
| `start <target>` | Starts the command of a dormant session again |
| `capture [-f] <target> [--lines <n>]` | Prints the last output lines of a session. With `-f`, then each byte that the command writes |
| `restore-policy <target> <policy>` | Sets when a dormant session starts again: `auto`, `on-focus` or `manual` |
| `detach <target>` | Detaches the clients that are attached to a session. The session continues |
| `hibernate <target>` | Stops the idle agent of a session and keeps the session hibernated, with its output and its resume command |
| `wake <target>` | Starts the agent of a hibernated session again with its resume command. `attach` does the same |
| `pin <target>` | Excludes a session from automatic hibernation |
| `unpin <target>` | Takes the exclusion of `pin` back |
| `kill <target>` | Ends a session and waits until its command has ended. Removes a dormant session with its output |
| `kill --all [--include-dormant] [--dry-run] [--yes]` | Ends each session whose command runs. On a terminal it asks first |
| `rename <target> <new-name>` | Gives a session a different name |
| `send [--no-enter] [-k] <target> <text>...` | Types a line, or keys with `-k`, into a session |
| `daemon status` | Shows if the daemon runs, and if the operating system applies a memory limit to a session |
| `daemon stop` | Stops the daemon. The sessions continue |
| `daemon install-service [--dry-run]` | Writes the file that starts the daemon at login |
| `daemon uninstall-service [--dry-run]` | Removes that file |
| `host add <ssh-target> [--name <name>]` | Connects to a host through SSH, installs nomux there, starts the daemon and saves the host |
| `host ls` | Lists the saved hosts |
| `host rm <name>` | Removes a saved host. Nothing changes on the host |
| `host doctor [<name>]` | Checks the connection, the platform, the binaries, the permissions and the daemon of a host, shows if its operating system applies a memory limit and what it applies of a sandbox, and checks the free space and the build output of the worktrees. Without a name, also the local host |
| `ext ls` | Lists the installed extensions |
| `ext install <source> [--grant <permission>]... [--grant-all] [--yes]` | Installs an extension from a directory on the host, a release archive, an `https` address of one, or the short form of an extension of `nomux-sh/extensions` (`git`, `git@0.1.0`), with the permissions that the options approve |
| `ext rm <id>` | Stops an extension and removes its package, its state and its data |
| `ext enable <id>` | Enables an extension, also after the daemon disabled it because of failures |
| `ext disable <id>` | Disables an extension and stops its process |
| `ext call <id> <method> [--payload <text> \| --payload @<file>]` | Calls a method of an extension and writes the payload of its result to the standard output |
| `ext validate <directory>` | Checks the `extension.toml` of a directory on this computer. The command needs no daemon |
| `project add [<directory>] [--name <name>] [--group <node>]` | Adds a directory of the host as a Project of the workspace tree |
| `project rm <node>` | Removes a Project from the workspace tree. No file and no session changes |
| `project ls` | Lists the Projects |
| `group new <name> [--in <node>]` | Makes an empty Group |
| `group rename <node> <name>` | Gives a Group or a Project a new name |
| `group move <node> [--to <node> \| --top] [--before <node> \| --after <node>]` | Puts a Group or a Project in a different Group, at the top level, or at a different position |
| `group rm <node>` | Removes a Group. Its content takes its place |
| `group ls [--tree]` | Lists the Groups. With `--tree`, the full tree with the Projects |
| `worktree ls [<node> \| --all] [--wait] [--fetch]` | Lists the git worktrees of a Project with their state, size and the reason why each one can or cannot be removed. The command only reads, except the `git fetch --prune` of `--fetch` |
| `worktree clean [<node> \| --all] [--build-output \| --finished \| --stale \| --prune] [--worktree <w>]... [--delete-branch] [--allow-unpushed] [--include-ignored] [--now] [--dry-run] [--yes] [--fetch]` | Removes build output and moves finished worktrees to the trash place. Without `--yes` it shows the plan, and asks on a terminal |
| `worktree restore <name>` | Moves a worktree back from the trash place during its grace time |
| `worktree trash ls [<node>]` | Lists the worktrees in the trash place |
| `worktree trash empty [<id>]... [--yes]` | Removes worktrees of the trash place now |
| `worktree mark <w> done\|keep`, `worktree unmark <w>` | Marks a worktree as finished, or to keep: no clean changes it. Removes the mark |
| `status [--tree]` | Shows the status of each session: what works, what is done and what needs the user. With `--tree`, also the Child work and the sessions below their Groups and Projects |
| `status set <state> [<message>]` | Reports the status of the session of this process: `working`, `done`, `blocked`, `error` or `idle`. Only inside a session |
| `agent explain <target>` | Shows what each source says about the status of a session |
| `events -f [--filter <type>]...` | Prints the events of the host when they come, until a signal stops the command |
| `hook <tool> <event>` | Sends one event of a hook of an agent tool to the daemon of the session. A tool calls it from its hooks. Only inside a session |
| `hooks install <tool> [--dry-run \| --apply]` | Prints the change that the hooks of a tool need in its settings file. Writes it only with `--apply`. Only on this computer |
| `hooks uninstall <tool> [--dry-run \| --apply]` | Prints how the removal of the hooks of a tool changes its settings file. Writes it only with `--apply`. Only on this computer |
| `hooks status` | Shows each tool with its adapter, and if its hooks are installed, missing or different |
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

### Targets and names

A session has an identifier, which the daemon gives and which never changes, and it can have a name, which the user gives and can change. Each command that takes a session takes a **target**: the identifier, the name, or the start of one of the two.

| Step | The target is | Sessions |
|---|---|---|
| 1 | the identifier of a session | each session, also an ended session |
| 2 | the name of a session | the sessions that are not ended |
| 3 | the start of the identifier or of the name of exactly one session | the sessions that are not ended |

The first step that finds a session wins: an exact name wins over a longer name with the same start. If step 3 finds more than one session, the command does nothing, prints the candidates and has the exit code 1. With `--json`, the error document has the field `candidates`. If no step finds a session, the exit code is 2.

```console
$ nomux new -n api
$ nomux new -n api-v2
$ nomux capture ap
nomux: ap can be more than one session. Give more of the identifier or of the name of one of them:
  okjzhnzv  api-v2  /bin/sh
  ouldrfi3  api     sleep 600
```

The rules of a name:

- A name has 1 to 64 characters from `A-Z`, `a-z`, `0-9`, `.`, `_` and `-`. The first character is a letter or a digit. Names are case-sensitive.
- A name is unique on the host among the sessions that are not ended. A dormant session keeps its name, also after a restart of the host. An ended session keeps its name in `ls --all`, and a new session can take that name at once.
- A name is not the identifier of a different session.
- `-t <target>` is the same as the argument, for the habit of tmux: `nomux attach -t api`. The help does not show it. `restore-policy` does not have it.
- A daemon that is older than the names refuses `new -n` and `rename` with the exit code 1. A target finds the sessions of such a daemon by their identifiers.

The CLI finds the session of a target with one `Session.List` request before the request of the command: `nomux-client` has the rule (`resolve_session`), thus each client finds a session in the same way.

### `new`

- `-n, --name <name>` is the name of the session: see "Targets and names". A name that a session that is not ended has gives the exit code 1, and the message names that session and `-A`.
- `-a, --attach` attaches the terminal to the new session, as `attach` does. The command then prints the result of the attach and not the session identifier. Without a terminal, the command fails and starts no session.
- `-A, --attach-or-create` needs `-n`: if a session that is not ended has the name, the command attaches to it, else it starts the session and attaches to it. The daemon decides it in one step, thus two `nomux new -A -n work` at the same time start one session. For a session that exists, the command and `--cwd` are not used, and one line on the standard error says so. A dormant session starts again, as `attach` does.
- `-c, --cwd <dir>` is the working directory of the command. The default is the current directory of the CLI. A relative path is relative to that directory.
- `--sandbox <profile>` is the sandbox profile of the session: `project`, `project-offline`, `read-only`, `off`, or a profile of the configuration file of the host. Without it, the host uses its default: [sandbox.md](sandbox.md) has the profiles and their rules. A profile that does not exist or that the host cannot apply starts no session, and the exit code is 1. A daemon that is older than the sandbox gives the exit code 1 too: it would start the session without limits.
- `--attach` attaches the terminal to the new session, as `attach` does. The command then prints the result of the attach and not the session identifier. Without a terminal, the command fails and starts no session.
- `-- <command> [argument...]` is the command of the session. The default is the shell of the user.
- `--cmd <command> [argument...]` is a second form of the same.

**Each argument after `--` or `--cmd` belongs to the command of the session, also an argument that is an option of `nomux`.** Thus the options of `nomux` come before the command:

```sh
nomux new
nomux new --json -- sleep 300            # --json is an option of nomux
nomux new -- sleep 300 --json            # --json is an argument of sleep
nomux new --cwd ~/project -- vim -u NONE notes.txt
nomux new --attach -- htop
nomux new -n api -c ~/src/api -- cargo watch -x test
nomux new -n api -a                      # start the session "api" and attach to it
nomux new -A -n work                     # attach to "work", or start it and attach
nomux new --sandbox project -- claude    # the agent can change only the project
```

**The two forms cannot be used together.** A command line that has `--cmd` and then `--` is a usage error with the exit code 1, and no session starts: `nomux` cannot know if the `--` is an argument of the command or the start of the second form.

```console
$ nomux new --cmd sh -- -c true
error: the argument '--cmd <COMMAND>...' cannot be used with '--': nomux cannot know where the command of the session starts. Give the command in one form: `nomux new --cmd <command> [argument...]` or `nomux new -- <command> [argument...]`. For a command that has `--` as an argument, use the second form

Usage: nomux new [OPTIONS] [-- <COMMAND>...]

For more information, try '--help'.
```

Thus a command that has `--` as one of its arguments needs the second form. After the first `--`, each `--` belongs to the command:

```sh
nomux new -- git log -- notes.txt        # the second -- is an argument of git
```

The session does not need the CLI or the daemon after that.

### `ls`

`ls` lists the sessions that can run: the sessions whose command runs, the dormant sessions and the damaged sessions. `ls --all` (`-a`) also lists the ended sessions, which the daemon keeps for the retention time. "Output" has the states.

If a session has a name, the table has the column `NAME` after `ID`, and `-` for a session without a name.
If a session belongs to a Project of the workspace tree, the table has the column `PROJECT` with the path of the Project, and `-` for a session without a Project. [workspace.md](workspace.md#the-project-of-a-session) has the rule.

If a session has a sandbox, the table has the column `SANDBOX` after `PID` with the profile, `*` after a profile in the state `partial`, and `-` for a session without a sandbox. The note of a partial session names the features that do not apply. [sandbox.md](sandbox.md#how-to-see-that-a-session-is-sandboxed) has the states.

A hibernated session has the state `hibernated`, and its note has the reason of the hibernation (`count`, `session-memory`, `total-memory`, `memory-pressure` or `request`) and the reason of a wake that failed. A pinned session has the note `pinned`.

`ls --memory` also shows the memory that the processes of each session use at the time of the command. The daemon measures the memory for this command only: without `--memory`, `ls` measures nothing.

```console
$ nomux ls --memory
ID        STATE       MEMORY     PROCESSES  HOLDER     MEASURE  LIMIT    FREED      COMMAND
k3xq2abc  running     512.4 MiB  7          607.0 KiB  pss      4.0 GiB  -          claude
p2rs6tuv  dormant     -          -          -          -        -        -          vim notes.txt
q4nn7abc  hibernated  -          -          -          -        -        487.1 MiB  claude --resume 0b1c
total: 512.4 MiB in 7 processes of 1 session, and 607.0 KiB in the holders (pss)
hibernated: 487.1 MiB given back by 1 session
```

| Column | Meaning |
|---|---|
| `MEMORY` | The memory of the process tree of the command: the command and each process that descends from it |
| `PROCESSES` | The number of processes in that tree |
| `HOLDER` | The memory of the holder of the session. It is not a part of `MEMORY` |
| `MEASURE` | `pss`: the proportional set size, which counts a page that more than one process uses one time. `rss`: the resident set size, which counts such a page for each process. Linux gives `pss`, macOS gives `rss` |
| `LIMIT` | The memory limit that the operating system applies to the processes of the command. `-` if none applies |
| `FREED` | For a hibernated session: the memory that its hibernation gave back. The last line has the sum, if a session is hibernated |

- A session without a process has `-` in the five columns of the measured values and is not in the total line.
- On a narrow terminal, the table leaves out `MEASURE` first, then `LIMIT`, `FREED`, `PROCESSES` and `HOLDER`: the total line names the measure, and the last line has the sum of `FREED`. See "Rules".
- The table has no `NAME` column, no `PID` column and no `NOTE` column. `ls` without `--memory` has them. The JSON document has the name.
- A size has the unit `KiB`, `MiB` or `GiB` (1024 bytes, 1024 KiB, 1024 MiB) and one decimal place. The JSON output has the bytes.
- If the sessions do not have the same measure, the total line says `pss and rss`: the sum then has values of the two kinds.
- A daemon that is older than this option answers with an error, and the exit code is 1.
- With `--host`, the daemon of the remote host measures the processes of that host.

[memory.md](memory.md) says how the daemon measures, what the values do not count, and the time of the command.

### Dormant sessions: `start` and `restore-policy`

A session is dormant when its holder is gone and its command did not end. After a restart of the host, each session is dormant: `ls` shows it with its command, and no process of it runs. [daemon.md](daemon.md) has the details.

- `attach <target>` on a dormant session starts its command again and attaches to it. The command starts in the working directory of the session, with the window size of the terminal. The processes of the session from before do not come back.
- `start <target>` starts the command again without an attach, with the window size that the session had. For a session whose command runs, it succeeds and changes nothing.
- `restore-policy <target> <policy>` sets when the command starts again:

| Policy | The command starts again |
|---|---|
| `auto` | When the daemon starts, for example at login. For a development server or a watcher |
| `on-focus` | At the first `attach` or `start`. This is the default |
| `manual` | Only with `start`. `attach` fails with the exit code 1 |

The default policy is `default_policy` in the `[restore]` table of the configuration file of the host. `ls --json` shows the policy of each session.
The command of an ended session does not start again: `start` and `attach` have the exit code 2 for it.

### Hibernation: `hibernate`, `wake`, `pin` and `unpin`

A hibernated session is an agent session whose agent the daemon stopped, because the agent was idle and its adapter gave a resume command. The session keeps its output and its last Agent run. [memory.md](memory.md#hibernation) has the triggers and the conditions.

- `hibernate <target>` hibernates the session now and prints the memory that the hibernation gave back. It fails with the exit code 1 and the reason if the session cannot hibernate: no adapter reports it as an agent session, its adapter gave no resume command, its agent works or waits for the user, a client shows it, or input came after the last change of its status. The idle time and the pin do not apply.
- `wake <target>` starts the resume command of the hibernated session in its working directory and prints the command. `attach <target>` does the same and shows the old screen first. A session that is not hibernated fails with the exit code 1: `start` starts a dormant session. A wake that fails has the exit code 1, the reason and the resume command to run by hand; the session stays hibernated.
- `pin <target>` excludes the session from each automatic hibernation. `unpin <target>` takes the exclusion back. `ls` shows the note `pinned`. A pin has no effect on `hibernate`.

### `capture`

`capture <target>` prints the last 100 lines that the command of the session wrote. `--lines <n>` sets the number.

- The lines come from the journal of the session. Thus the command also gives lines that left the screen, lines from before a restore, and the lines of a dormant or an ended session.
- A line ends with a line feed. The output is the bytes as the command wrote them: it has the escape sequences of the command, and a terminal session ends a line with a carriage return and a line feed.
- If the session has fewer lines, the command prints all of its output.
- With `--quiet`, the output is the same. With `--json`, the output is a string: see "JSON output".
- `-f, --follow` prints the lines, and then each byte that the command writes, until the session ends, a signal stops the command (`SIGINT`, `SIGTERM`), or the reader closes the output: then the exit code is 0. Thus `nomux capture -f api >> api.log` writes the output of a session to a file. The command attaches without a window size and without input, as a read-only client, from the end of the lines: it gets each byte one time and no screen. It has no JSON form.

### `send`

`send <target> <text>...` writes the words of the text with one space between them to the terminal of the session, and then Enter (a carriage return), as if the user typed them. The bytes go in one request, thus they stay together. A script uses it to give a command to a shell or a prompt to an agent:

```sh
nomux send api 'cargo test'               # types it and presses Enter
nomux send -k api C-c                     # interrupt
nomux send --no-enter api 'draft' && nomux send -k api Enter
```

- `--no-enter` sends the text without Enter.
- `-k, --keys`: the words are key names, without Enter after them: `Enter`, `Escape`, `Tab`, `S-Tab`, `Space`, `BSpace`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PgUp`, `PgDn`, `F1` to `F12`, `C-<letter>` and `M-<character>` (the names of tmux). A cursor key has the sequence of the normal cursor mode. A name that the command does not know gives the exit code 1 and sends nothing.
- A text with a line break gives the exit code 1 and sends nothing: send one line at a time.
- `send` writes at once: it does not wait until the program is ready. A dormant session gives the exit code 1 (`send` does not start a session), an ended session the exit code 2.
- The output is empty. With `--json`: see "JSON output".

### `attach`

`attach [<target>]` makes the terminal of the user the terminal of the session:

1. The session gets the window size of the terminal.
2. The terminal shows the current screen of the session. A program that already drew its screen, for example an editor, is visible immediately. For a dormant session, the terminal first shows the end of the output from before, then the line `--- nomux: the session started again. The output before this line is from the command that stopped ---`, and then the screen of the new command. The old output and the line are in the scrollback of the terminal after that.
3. Each key goes to the session, and the terminal shows each byte that the session writes, except the queries that the session answers itself (for example `CSI c` and `CSI 6 n`): the terminal does not answer them a second time. The terminal is in raw mode: `Ctrl-C` and `Ctrl-Z` go to the programs of the session.
4. When the window gets a different size, the session gets the new size.

`<session>` in a message is the name of the session with its identifier, `api (m4ab5ghi)`, or only the identifier of a session without a name.

Without a target, `attach` takes the session that a client attached to last, else the session that started last, of the sessions that are not ended. The daemon keeps the time of the last attach in its memory: after a restart of the daemon, the session that started last. If the host has no such session, the exit code is 2. `a` is the same command.

- `-d, --detach-others` detaches the other clients of the session first, as `detach` does. A different client can attach between the two requests.
- `-r, --read-only` shows the session and sends no key and no window size to it: the session keeps its size. The detach keys work. The client keeps this rule itself; the daemon does not enforce it.

The command needs a terminal on its standard input. More than one terminal can be attached to one session at the same time: each one shows the same output, and each one can type.
The session has one window size: the size of the terminal that attached or changed its size last. A terminal with a different size shows the output as the programs made it for that size.

**Keys.** A prefix key comes before each key of `nomux`. The default prefix keys are `Ctrl-a`, the key of screen, and `Ctrl-\`, the key of nomux 0.4. The two are active at the same time.

| Keys | Function |
|---|---|
| A prefix and then `d` or `Ctrl-d` | Detach. The session continues. `Ctrl-d` is for a user who still holds `Ctrl` |
| A prefix two times | Send one prefix key to the session |
| `Ctrl-a` and then `a` | Send one `Ctrl-a` to the session, as in screen. For a different prefix of `Ctrl` and a letter, that letter |
| A prefix and then `?` | Show the keys in the title of the terminal window until the next key. The screen of the session does not change |
| A prefix and then a different key | Send the two keys to the session |

- A prefix delays a key until the next key, and it takes the keys of the table. Thus a lone `Ctrl-a` (start of the line in a shell) has its effect after the next key. `Ctrl-a` and then a different key, for example `Ctrl-a Ctrl-k`, works as without a prefix.
- **The setting.** The first of these that is set gives the prefix keys: the option `--prefix <key>` of `attach` and `new --attach` (two times for two keys), the variable `NOMUX_PREFIX` (keys with a comma between them: `C-b`, `C-a,C-\`), `prefix` in the `[attach]` table of the configuration file of the computer that runs the CLI (`prefix = ["C-b"]`), and the default. A remote host does not change the setting: the CLI reads the keys.
- A key is `C-<x>`: `x` is a letter, `\`, `]`, `^`, `_`, `@` or `Space`. `C-c`, `C-d`, `C-z`, `C-s`, `C-q`, `C-h`, `C-i`, `C-j`, `C-m` and `C-[` cannot be a prefix: programs need them. A list has two different keys at most. A setting that is not valid gives the exit code 1, and `new --attach` then starts no session.
- `none` (`--prefix none`, `NOMUX_PREFIX=none`, `prefix = "none"` or `prefix = []`) turns the prefix off: each key goes to the session. Detach with `nomux detach <id>` from a different terminal, or close the terminal.
- **Nested sessions.** In a session in a session, the outer client takes the prefix first. `Ctrl-a a d` detaches the inner client. Or start the inner client with a different prefix: `nomux attach --prefix C-b <id>`.
- **Key forms.** A program of the session can make the terminal send keys in the form of the kitty keyboard protocol (`CSI 97 ; 5 u` for `Ctrl-a`) or of `modifyOtherKeys` (`CSI 27 ; 5 ; 97 ~`). The CLI finds the prefix and the keys after it in the three forms, also when a read of the terminal divides a sequence. A release event of a key that the CLI took does not go to the session. Each other byte goes to the session as it came.
- The text of a bracketed paste has no keys of `nomux`: a pasted `Ctrl-a d` goes to the session.

`nomux attach --help` shows these keys.

**End.** An attach ends in one of these manners. The terminal gets its earlier mode back in each of them, also when the CLI gets a signal or fails.

| Event | Message | Exit code |
|---|---|---|
| The user detaches, `nomux detach` detaches the client, or the terminal closes | `detached from session <session>` | 0 |
| The command of the session ends | `session <session> ended with exit status <n>` | `<n>`: the exit status of the command |
| A signal ends the command of the session | `session <session> ended by signal <n>` | 128 + `<n>`, as a shell gives it |
| The session ends and the daemon cannot get the exit status | `session <session> ended; its exit status is not known` | 1 |
| The daemon stops or fails | An error that says that the session continues and how to attach again | 1 |
| The connection to a remote host ends | The attach connects again and continues with the output that it does not have. The title of the terminal window says `nomux: connection lost, connecting again` until then, and the keys of that time are discarded. After approximately 48 seconds without a connection: an error that says how to attach again | 1 after the error |
| The CLI gets `SIGTERM`, `SIGHUP`, `SIGINT` or `SIGQUIT` | An error | 1 |
| No session has the identifier, or the command of the session ended before the attach | An error | 2 |
| The session is dormant and has the restore policy `manual`, or its command cannot start | An error | 1 |

Thus the exit code of `attach` can be a value that the table "Exit codes" does not have, and the values 1 to 5 can come from the command of the session. With `--json`, the document of a session that ended goes to the standard output, and an error goes to the standard error.

The CLI does not use the alternate screen of the terminal: after the attach, the terminal shows the last screen of the session, and the message is on the next line.

### `rename`

`rename <target> <new-name>` gives the session the name `<new-name>`, with the rules of "Targets and names". The same name as now succeeds and changes nothing. A name that a different session has gives the exit code 1. An ended session cannot get a new name: the exit code is 2.

```console
$ nomux rename ouldrfi3 web
session ouldrfi3 is now named web
```

### `detach`

`detach <target>` detaches each client that is attached to the session, from a different terminal or from a script. Each of these clients ends as after the detach keys. The session continues.
The command succeeds also if no client is attached. If no session has the identifier, the exit code is 2.

A client detaches itself with the detach keys. The command has no form without an identifier yet: a later version takes the identifier from the environment of the session.

### `kill`

The command of the session gets `SIGHUP`. If it still runs after 2 seconds, it gets `SIGKILL`. The session is an ended session then: `ls --all` shows it with its exit status for the retention time.
If no session has the identifier, or if its command ended already, the exit code is 2. Thus the second `kill` of the same session has the exit code 2.

A dormant session and a damaged session have no command to end. `kill` removes such a session and its state: its output is gone after that.

`kill --all` ends each session whose command runs (the states `running` and `unreachable`), one after the other, and prints one line for each. It does not touch a dormant or a damaged session: after a restart of the host each session is dormant, and `kill --all` must not remove their output. `--include-dormant` also removes these sessions.

- On a terminal, the command first asks `end <n> sessions? [y/N]` on the standard error. Only `y` or `yes` ends them; each other answer ends none, with the exit code 1. `--yes` does not ask. Without a terminal, `--all` needs `--yes`: else the exit code is 1 and no session ends.
- `--dry-run` prints the sessions and ends none.
- If no session runs, the command prints `no session runs`, asks nothing and has the exit code 0. If a session does not end, its error goes to the standard error, the other sessions still end, and the exit code is the highest code of the failures.
- `kill` has no `-a`: in tmux, `kill-session -a` ends each other session.

### `daemon status` and `daemon stop`

`daemon status` has the exit code 0 if a daemon runs and the exit code 2 if no daemon runs.
On the local host, `daemon status` also shows the path of the global configuration file that the daemon reads at its start: [daemon.md](daemon.md#configuration). With `--host`, it does not show the path. The CLI computes the path from its own environment: a daemon that started with a different `HOME` or `XDG_CONFIG_HOME`, for example from a service file, can read a different file.
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

`host doctor <name>` checks one host, and `host doctor` checks the local host (with the name `local`: only `daemon`, `sandbox` and the checks of the worktrees) and each host of the configuration file. The command changes nothing on a host: it installs nothing and starts no daemon.

| Check | Passes if |
|---|---|
| `connection` | The SSH connection, the host key and the authentication are correct |
| `platform` | nomux has binaries for the operating system and the processor of the host |
| `binaries` | The host has the binaries of the version of the CLI |
| `permissions` | `~/.nomux`, `~/.nomux/bin`, `~/.nomux/run` and the socket belong to the user, other users cannot write to the first two, and other users have no access to the last two |
| `daemon` | A daemon answers, or no daemon runs. It fails if the SSH server refuses connections to Unix sockets |
| `versions` | The CLI and the daemon have a common protocol version. Only if a daemon answers |
| `sandbox` | Always passes, as `memory-limit` does: a host without a sandbox runs sessions. The detail has the operating system, the version of the Landlock interface, the features that it applies and the ones that it does not, and says when a session with a sandbox profile does not start on the host: [sandbox.md](sandbox.md#features). Only if a daemon of this version answers |
| `free-space` | The file systems of the Projects have 3 GiB free or more. Below 10 GiB the detail starts with `warning:`. Only if a daemon answers and the host has a Project |
| `shared-build-output` | Always passes. The detail has the advice of [worktrees.md](worktrees.md#commands) when two or more worktrees have their own cargo target directory or their own `node_modules`, with the measured numbers. Only if a daemon answers and the host has a Project |

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

#### From a release archive

`<source>` can also be a release archive. The command finds the form in this sequence:

| `<source>` | Archive |
|---|---|
| A text with `://` | The address of an archive. Only `https`, with a host, without a user name or a password, ending with `.tar.gz`. The checksum file is `SHA256SUMS` in the same directory of the server |
| A directory of this computer | No archive: the package directory, as above |
| A file of this computer | The archive. If a file `SHA256SUMS` is next to it, the command compares the checksum. If not, the summary says that the checksum was not compared |
| `<name>` or `<name>@<version>`: lower-case letters, digits and `-`, and a version `X.Y.Z` | The release `<name>@<version>` of [`nomux-sh/extensions`](https://github.com/nomux-sh/extensions) for the release target of the host: `https://github.com/nomux-sh/extensions/releases/download/<name>@<version>/nomux-<name>-<version>-<target>.tar.gz` and its `SHA256SUMS`. Without a version, the newest release `<name>@X.Y.Z` from the release list of the GitHub API |
| Each other text | The path of a package directory, as above |

The steps:

1. The command gets the archive and its checksum file, and compares the SHA-256 checksum before it opens the archive. With `--host`, the target is the target of the remote host.
2. It reads the archive in memory and refuses it, with nothing written, if an entry is not a regular file or a directory (a link, a device, a FIFO, a pax or GNU long name header), if a path is absolute, leaves the package directory or has a character other than letters, digits and `. _ + -`, if the archive has more than one package directory, more than 256 entries or more than 256 MiB of files, or if the package directory does not end with the release target of the host.
3. It prints a summary on the standard error: the archive, the identifier and the name of the extension, its version, the platform, the result of the checksum and the permissions that the extension asks for. Nothing is written before.
4. Without an approval of the permissions, it stops as above. Then it asks `Install ... with the approved permissions? [y/N]` on the terminal. `--yes` accepts without the question. Without a terminal and without `--yes`, nothing is written and the exit code is 4.
5. It writes the package into a new directory of `~/.nomux/downloads` that only the user can read, on the host of the extension (with `--host`, through the SSH connection), installs it as a directory package, and removes the directory. The daemon keeps its own copy.

The download uses the program `curl` of the user, with `--proto =https --proto-redir =https --tlsv1.2`, 10 redirects at most, a connection time of 15 seconds and a total time of 300 seconds at most, and 64 MiB at most for each file; `--disable` makes `curl` ignore a `.curlrc` file. The download files are in a temporary directory of `~/.nomux/downloads` with the mode 0600, not in `/tmp`. Without `curl`, download the archive by hand and give its path. No other command of nomux uses the network for an extension.

```console
$ nomux ext install git --grant-all --grant fs=/home/user/project
nomux: the archive https://github.com/nomux-sh/extensions/releases/download/git@0.1.0/nomux-git-0.1.0-x86_64-unknown-linux-musl.tar.gz
  extension:   nomux.git (Git)
  version:     0.1.0
  platform:    x86_64-unknown-linux-musl
  checksum:    the SHA-256 checksum is the one of SHA256SUMS
  permissions:
    fs=${worktree}
    process=git
Install nomux.git 0.1.0 with the approved permissions? [y/N] y
extension nomux.git 0.1.0 installed with the permissions: fs=/home/user/project process=git
```

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

### `project` and `group`

These commands change and list the workspace tree of the host: the Groups and the Projects. [workspace.md](workspace.md) has the model, the rules and examples.

- A `<node>` is the identifier of a node, or its path: the names from the top level down to it with `/` between them, for example `Client work/Acme/acme-web`. The name alone is also accepted if one node has it. If more than one node has the name or the path, the command fails with exit code 1 and lists the identifier and the path of each one. A node that does not exist gives exit code 2.
- `project add` without a directory adds the current directory. On the local host, a directory in a git repository adds the root of the repository. With `--host`, give the directory as an absolute path on the remote host. The daemon resolves the symbolic links of the directory and refuses a directory that a Project has already. The default name is the last part of the directory.
- `group move` needs `--to`, `--top`, `--before` or `--after`. `--before <node>` and `--after <node>` put the node next to a sibling: without `--to` and `--top`, the node goes to the Group of that sibling. A move of a Group into itself or into a Group below it fails with exit code 1.
- `group rm` removes only the Group: its Groups and Projects take its place in its parent.
- A change prints one line: what changed, the identifier, the path after the change and the revision of the tree. `--quiet` prints the identifier.
- The listings print a table: `project ls` the identifier, the path and the directory; `group ls` the identifier and the path; `group ls --tree` each node below its Group, indented two spaces for each level, with its kind, its identifier and the directory of a Project. `--quiet` prints the identifiers.
- A daemon that is older than these commands answers with an error, and the exit code is 1.

### `worktree ls`

`worktree ls <node>` lists the worktrees of one Project, `worktree ls --all` of each Project. Without both, on the local host, the Project of the current directory, or else of the main worktree of its git repository: a linked worktree can be outside the directory of its Project. With `--host`, a Project or `--all` is necessary. [worktrees.md](worktrees.md) has the model, the rules and how the daemon finds each fact. The git facts come from the git extension `nomux.git`: without it, or without an approval of the directory of the Project, the Project has no rows and a line below the table says what to do.

- The command only reads. It changes no file of a repository. The daemon and the git extension run no `git` command; only the CLI without a Project runs `git rev-parse --git-common-dir` to find the main worktree.
- The table has a row for each Project and below it, indented, a row for each worktree: `WORKTREE` (the last part of its directory), `BRANCH` (`-` for a detached `HEAD`), `STATE` (`active`, `finished`, `stale` or `idle`), `SIZE` and `BUILD` (the size and the part in build output, with one letter for the unit: `K`, `M`, `G`, `T`), `AGE` (the time since the last activity) and `REASON`: `yes:` and the merge rule, or `no:` and the first reason why a removal of the directory is refused, for example `no: unpushed 2` or `no: ignored files (.env)`.
- A size that the daemon did not measure yet is `?`, and an old size has `*` after it. The daemon measures them in the background; `--wait` waits until each size is measured.
- `TOOL` is the agent tool that probably made the worktree (`claude-code`, `codex`, `cursor`, `orca`), from its directory, or `-`.
- On a terminal, the table fits its width: `BRANCH`, `WORKTREE` and `REASON` lose their end, then `TOOL`, `BUILD` and `AGE` go away.
- `--fetch` runs `git fetch --prune --quiet` in the directory of each listed Project first: the only command of nomux that uses the network. Only on the local host. A fetch that fails prints a warning, and the list follows.
- The last line has the number of worktrees, their size and build output, the worktrees that a removal can take and their size, and for one Project the free space and the age of the last fetch. Then a line for each Project or worktree without git facts.
- `--quiet` prints the directory of each worktree.
- A daemon that is older than this command answers with an error, and the exit code is 1.

### `worktree clean`

`worktree clean` asks the daemon for a plan and runs exactly the items of that plan. The Projects are as for `worktree ls`. [worktrees.md](worktrees.md#the-clean) has the sequence, the refusals and how to undo each step.

| Option | Items |
|---|---|
| `--finished` (default) | For each finished worktree: its build output, then the move of its directory to the trash place; and the prune of the records of missing directories |
| `--build-output` | The build output of each linked worktree. The main worktree only when `--worktree` names it |
| `--stale` | The build output of each stale worktree |
| `--prune` | Only the prune of the records of missing directories, or a `repair` proposal |
| `--worktree <w>` | Only this worktree: its path, or the last part of its path. Give the option more than one time for more worktrees |
| `--delete-branch` | At the end of the grace time, also delete the branch when the base branch has each change of it |
| `--allow-unpushed` | Also move a worktree with commits that are not pushed. The branch keeps the commits |
| `--include-ignored` | Also move the ignored files that are not build output. They are deleted at the end of the grace time |
| `--now` | Remove each worktree now, without the grace time. Also a worktree without a trash place |
| `--dry-run` | Show the plan and change nothing |
| `--yes` | Run the plan without a question |
| `--fetch` | `git fetch --prune` in each Project first, as for `worktree ls` |

- The table has a row for each item: `WORKTREE`, `STEP` (`build output`, `to trash`, `delete now`, `prune record` or `repair`), `PATH` (relative to the worktree, or to its parent directory for the trash place), `SIZE` (an upper bound) and `REASON`: `yes:` and the rule or what happens to the branch, or `no:` and the first refusal. The last line has the number of items that run and the bytes that come back.
- Without `--yes`, on a terminal, the command shows the plan on the standard error and asks `Clean <n> item(s), <size> (upper bound)? [y/N]`. Each answer but `y` changes nothing. Without a terminal, and with `--dry-run`, the command shows the plan and changes nothing.
- After a clean, the table has the result of each item: `done`, `skipped:` with the reason when the facts changed after the plan, `no:` with the refusal, or `failed:` with the error. The last line has the bytes of the plan and the increase of the free space.
- The exit code is 0 when each item ran, and 1 when an item was refused, skipped or failed. A plan without a clean (`--dry-run`) has the exit code 0.

```console
$ nomux worktree clean acme --yes
WORKTREE    STEP          PATH                              SIZE   REASON
acme-dirty  build output  target                            2.8M   no: uncommitted changes
acme-dirty  to trash      .nomux-trash/acme-dirty-3dc4ed24  20.0K  no: uncommitted changes
acme-done   build output  target                            2.8M   done
acme-done   to trash      .nomux-trash/acme-done-078ea7a4   20.0K  done
acme-env    to trash      .nomux-trash/acme-env-1c7fbf68    24.0K  no: ignored files (.env)
acme-gone   prune record  -                                 -      done
3 of 6 items done: 2.8M planned, the free space grew by 2.9M.
```

### `worktree restore`, `worktree trash`, `worktree mark`

- `worktree restore <name>` moves the worktree back to its directory and unlocks it. `<name>` is the identifier of the trash entry, the directory of the worktree, or its last part when one entry has it. A directory that exists again is an error. Build output does not come back.
- `worktree trash ls [<node>]` has a row for each entry: `ID`, `WORKTREE` (the directory before the move), `BRANCH`, `MOVED` and `REMOVED IN` (the time that is left of the grace time). `--quiet` prints the identifiers.
- `worktree trash empty [<id>]...` removes the entries now, or each entry without an identifier, with the checks of the end of the grace time. It asks on a terminal; without a terminal it needs `--yes`. An entry that stays has a line with the reason, and the exit code is 1.
- `worktree mark <w> done` makes a worktree finished; its branch stays unless the base branch has each change. `worktree mark <w> keep` protects a worktree from each clean. `worktree unmark <w>` removes the mark. `<w>` is the directory or its last part.

### `status`

`status` prints one row for each session that is not ended, and for each ended session that keeps its last Agent run ([status.md](status.md#ended-sessions)): a mark of the state, the session identifier, the command, the state, the time in the state and the message. [status.md](status.md) has the model and the sources of a status.

- The state of a row is the most urgent status of the session and of its Child work. `needs you` has the kind of the wait if a source says it: `needs you: permission`.
- A status that the daemon guessed has `(guess)`: the prompt marks of the shell or the foreground program of the terminal, and no report of a program.
- A result that the user saw has `(seen)`. An attach to a session marks its results as seen.
- A hibernated session has `hibernated: <reason>` as its state, for example `hibernated: count`. The JSON output has the field `hibernated` with the reason in the object of the session.
- The mark: `!` needs you, `x` error, `+` done, `*` working, and no mark for idle.
- With `--tree`, each Child work is a row below its session, indented two spaces for each level. A Child work that no source reports has `-` as its state.
- A session that has a name shows its name in place of its identifier.
- With `--tree`, the sessions without a Project come first. Then each Group (its name ends with `/`) and each Project of the workspace tree is a row, in the sequence of the tree and indented for each level, with the most urgent status of the sessions below it ([workspace.md](workspace.md#status)). The sessions of a Project are the rows below it. A Group or a Project without a session has `-` as its state. The first column has the header `NAME`.

```console
$ nomux status --tree
  NAME            COMMAND    STATE                  FOR  MESSAGE
* p2rs6tuv        make test  working (guess)        2m
! Client work/                needs you: permission  1m   run tests?
!   acme-web                  needs you: permission  1m   run tests?
!     k3xq2abc    claude     needs you: permission  1m   run tests?
    notes                     -                      -
```

### `status set`

`status set <working|done|blocked|error|idle> [<message>]` reports the status of the session that the command runs in. A script or a tool that does not send OSC 7501 uses it.

- The command reads `NOMUX_SESSION_ID`, `NOMUX_SOCK` and `NOMUX_TOKEN`, which the holder gives to each process of a session. It connects to the socket of `NOMUX_SOCK`, and it does not start a daemon. `--host` is not permitted.
- `blocked` is the state `needs you`. `idle` takes the report back.
- The message is one line of 512 bytes at most, without a control character.
- Exit code 4: a variable is not set (the command does not run in a session), the token is wrong, the session has no process, or its holder is too old to make a token.

### `agent explain`

`agent explain <target>` prints the status of a session with its source, and then each source in the sequence in which they decide, with what it says and why: the note of the source. A source that reports nothing is in the list too.

For an ended session, the command prints its kept run, with the resume command of the tool, until the session is removed. An ended session without a kept run gives exit code 2.

### `events --follow`

`events --follow` (`-f`) subscribes to the events of the host and prints each event when it comes: with `--json`, one JSON document on each line (NDJSON). The command ends with the exit code 0 on `SIGINT` (Ctrl-C) or `SIGTERM`, when the daemon closes the connection, and when the reader of the standard output closes it.

- `--filter <type>` prints only the events of a type: `agent-run-changed`, `agent-run-removed`, `extension-event`, `workspace-changed`, `session-hibernated`, `session-woken`, `session-wake-failed` or `session-memory-limit-reached`. Give the option more than one time for more types. A snapshot is always printed.
- The command prints the events after its start. A snapshot comes when the daemon does not have the events that the command needs.
- A text line is `<seq> <session> <state> <message>` for a change, `<seq> <session> removed` for a session that ended or was removed, `<seq> extension <id>: <name> (<n> bytes)` for an event of an extension, `<seq> workspace revision <revision>: <change>` for a change of the workspace tree, and `<seq> snapshot: ...` for a snapshot.

### `hook`

`hook <tool> <event>` is the command that an agent tool calls from its hooks. [adapters.md](adapters.md) describes the path of an event to the adapter extension of the tool.

- The command reads the data of the event from the standard input: 256 KiB at most, until its end or for 40 ms at most. A larger input goes without its data. A terminal is no input.
- It reads `NOMUX_SESSION_ID`, `NOMUX_SOCK` and `NOMUX_TOKEN`, connects to the socket of `NOMUX_SOCK` and sends one `Hooks.Event` request. It does not start a daemon, a shell or a different process. `--host` has no effect.
- It writes the answer of the daemon to the standard output as it is: the answer for the tool. In this version the answer is empty.
- A hook must not block or break its tool: the exit code is always 0. Outside a session, without a daemon, and when the daemon does not answer in 80 ms after the start of the command, the command ends with no output. A refusal of the daemon, for example a wrong token, writes one line to the standard error.

### `hooks install`, `hooks uninstall` and `hooks status`

These commands change and show the hooks of an agent tool in the settings file of the tool on this computer. The adapter extension of the tool describes the entries that its hooks need. `--host` is not permitted: the settings file of a tool is on the computer of the tool.

- `hooks install <tool>` asks the adapter for its install plan, computes the new content of the settings file and prints it as a unified diff. It writes nothing. `--dry-run` is the default and says it again. With `--apply`, the command writes the file: a temporary file in the same directory replaces it, and a symbolic link stays a link. The install changes only the text that it adds, at the positions of the values in the file: each other byte stays, with the order of the keys, the indent, the spacing and the end of the file. A new entry gets the indent of the file (tabs or spaces), or stays on one line in an object or a list on one line. A value that the file has already is not added again.
- `--apply` also writes a record to `hooks/<tool>/` in the data directory of nomux (`~/.local/share/nomux` on Linux, `~/Library/Application Support/nomux` on macOS): the bytes of the file before the install, the checksum of the written file and the entries.
- `hooks uninstall <tool>` needs only the record, not the daemon. If the file did not change after the install, the file gets its bytes from before the install again, or is removed if it did not exist. If the file changed, only the entries of nomux go away, with each list and object that the install made and that has no other value then. A list or an object that the file had before the install stays. The command prints the diff and writes only with `--apply`. Exit code 2: nomux did not install the hooks of the tool.
- `hooks status` lists each tool that has an adapter or a record: the adapter, the state of the hooks in the file (`installed`, `missing` or `different`) and the path of the file.
- nomux never writes a settings file without `--apply`, never wraps or replaces the command of a tool, and never adds an option that skips the permission checks of a tool.

## Output

### Text

```console
$ nomux new
k3xq2abc
$ nomux new -n api
m4ab5ghi
$ nomux ls
ID        NAME  STATE    PID    COMMAND
k3xq2abc  -     running  41873  /bin/zsh
m4ab5ghi  api   running  41880  /bin/zsh
$ nomux attach api
detached from session api (m4ab5ghi)
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
configuration file: /home/user/.config/nomux/config.toml
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
| `worktree ls` | The directory of each worktree on a line |
| `worktree clean` | Each path that the plan deletes or moves, or that the clean deleted or moved, on a line |
| `worktree trash ls` | One trash entry identifier on each line |
| `worktree restore` | The directory of the restored worktree |
| `status` | One identifier on each line of a session that needs the user |
| `events --follow` | The lines of the text output |
| `ext call` | The payload of the result, as without `--quiet` |
| `version` | The version number |
| `capture -f` | The output, as without `--quiet` |
| `new --attach`, `attach`, `start`, `restore-policy`, `detach`, `kill`, `rename`, `send`, `daemon status`, `daemon stop`, `daemon install-service`, `daemon uninstall-service`, `host add`, `host rm`, `host doctor`, `ext install`, `ext rm`, `ext enable`, `ext disable`, `ext validate`, `status set`, `agent explain` | Nothing. The exit code is the result |

### Errors

An error goes to the standard error as `nomux: <message>`, and the standard output stays empty. The text of a message is not stable: a script must use the exit code.
A wrong command line prints the usage message of the argument parser and has the exit code 1.

## Exit codes

| Code | Name in JSON | Meaning |
|---:|---|---|
| 0 | | Success |
| 1 | `error` | An error that has no other code, also a wrong command line, a target that can be more than one session, a session name that is not valid or in use, an SSH connection that cannot be made, a host key that is not trusted, a check of `host doctor` that failed, a dormant session that cannot start, a manifest that is not valid, and an extension that failed |
| 2 | `not_found` | The item does not exist: no session has the identifier (no session matches the target), the command of the session ended, no host has the name, no extension has the identifier, or no daemon runs (`daemon status`) |
| 3 | `timeout` | An operation did not complete in the permitted time: the daemon did not answer after its start, a session was not ready, a command did not end, or an extension did not answer |
| 4 | `no_permission` | The user does not have permission, for example for the socket of the daemon, or a remote host accepted no key of the user. Also: `ext install` without an approval of the permissions, an extension that does not have a permission, and a `status set` that cannot prove its session |
| 5 | `version_mismatch` | The CLI and the daemon have no common protocol version. Update the older of the two |

The codes 1 to 5 are the values of `ErrorCode` in the control protocol.

`attach` and `new --attach` have one more source of the exit code: if the session ends during the attach, the exit code is the exit status of its command. "`attach`" has the table.

## JSON output

With `--json`, a command prints one JSON document on one line.

- On success, the document goes to the standard output.
- On an error, the standard output stays empty and the document goes to the standard error. It has the field `error`.
- Each document has the field `schema_version`. The current value is `1`.

**Schema version rule.** The value increases when a field is removed, gets a different name or gets a different meaning. A new field does not increase the value, thus a reader must ignore the fields that it does not know.

### `new`, `start`, `hibernate` and `wake`

```json
{"schema_version":1,"session":{"id":"k3xq2abc","name":"api","state":"running","pid":41873,"command":["/bin/zsh"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus"}}
```

| Field | Type | Meaning |
|---|---|---|
| `session.id` | string | The session identifier |
| `session.name` | string or `null` | The session name. `null` if the session has no name, and from an older daemon |
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
| `session.hibernation` | object | Only if the session hibernated: `hibernated_at` (seconds after the Unix epoch), `reason` (`count`, `session-memory`, `total-memory`, `memory-pressure`, `request` or `unknown`), `freed_bytes` and `measure` (`null` if the daemon did not measure), `resume_command`, `woken_at` (`null` while the session is hibernated; else the time of the wake of the current run) and `wake_error` (`null`, or why the last wake failed). A hibernated session has the `state` `dormant` |
| `session.pinned` | boolean | Only if it is `true`: the session is excluded from automatic hibernation |
| `session.sandbox` | object | Only for a session with a sandbox: `profile`, `state` (`enforced`, `partial`, `inactive` for a session without a process, or `unknown`), `enforced` and `missing` (arrays of the feature names `files`, `network_off`, `unix_sockets`, `signals` and `abstract_sockets`) |

A field is `null`, or `command` is empty, when the daemon does not know the value: for a damaged session, and for a session of a holder that is too old to write the description. The document never has the environment of the command.

### `ls`

```json
{"schema_version":1,"sessions":[{"id":"k3xq2abc","name":null,"state":"dormant","pid":null,"command":["/bin/zsh"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus"}]}
{"schema_version":1,"sessions":[{"id":"k3xq2abc","name":null,"state":"ended","pid":null,"command":["make"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus","exit":{"ended_at":1795651260,"exit_code":2}}]}
```

`sessions` is an array of the object that `new` gives, in the sequence of the identifiers, with one more field: `project`, the Project of the session (`id` and `path`), or `null` if the session has no Project. It is empty if there is no session. An ended session is in the array only with `--all`.

### `ls --memory`

```json
{"schema_version":1,"sessions":[{"id":"k3xq2abc","name":null,"state":"running","pid":41873,"command":["claude"],"cwd":"/home/user/project","rows":24,"columns":80,"started_at":1795651200,"epoch":1,"restore_policy":"on-focus","memory":{"command_bytes":537290342,"command_processes":7,"holder_bytes":621568,"measure":"pss"}},{"id":"p2rs6tuv","name":null,"state":"dormant","pid":null,"command":["vim","notes.txt"],"cwd":"/home/user","rows":24,"columns":80,"started_at":1795651100,"epoch":1,"restore_policy":"on-focus","memory":null}],"memory_total":{"command_bytes":537290342,"command_processes":7,"holder_bytes":621568,"sessions":1,"measure":"pss"}}
```

The document is the document of `ls` with one more field in each session and one more field at the top. Without `--memory`, the document has neither of the two.

| Field | Type | Meaning |
|---|---|---|
| `sessions[].memory` | object or `null` | The memory of the processes of the session. `null` if the session has no process |
| `sessions[].memory.command_bytes` | number | The memory of the process tree of the command in bytes, without the holder |
| `sessions[].memory.command_processes` | number | The number of processes in that tree |
| `sessions[].memory.holder_bytes` | number | The memory of the holder of the session in bytes |
| `sessions[].memory.measure` | string | `pss`, `rss`, or `unknown` for a measure that the CLI does not know |
| `sessions[].memory.limit_bytes` | number or `null` | The memory limit that the operating system applies to the command. `null` if none applies |
| `memory_total.command_bytes`, `memory_total.command_processes`, `memory_total.holder_bytes` | number | The sums of the three values of the sessions that have a process |
| `memory_total.sessions` | number | The number of sessions in the sums |
| `memory_total.measure` | string or `null` | The measure of the sums. `null` if the sessions do not have the same measure, or if no session has a process |
| `memory_total.freed_bytes`, `memory_total.hibernated_sessions` | number | The memory that the hibernated sessions gave back, and their number |

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

### `attach`, `new --attach` and `new -A`

```json
{"schema_version":1,"attach":{"id":"k3xq2abc","name":"api","end":"detached"}}
{"schema_version":1,"attach":{"id":"k3xq2abc","name":null,"end":"exited","exit_code":0}}
{"schema_version":1,"attach":{"id":"k3xq2abc","name":null,"end":"signaled","signal":9}}
{"schema_version":1,"attach":{"id":"k3xq2abc","name":"work","created":false,"end":"detached"}}
```

The command prints the document after the attach ended.

| Field | Type | Meaning |
|---|---|---|
| `attach.id` | string | The session identifier |
| `attach.name` | string or `null` | The session name |
| `attach.created` | boolean | Only for `new -A`: `false` if the session existed |
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

### `rename`

```json
{"schema_version":1,"renamed":{"id":"k3xq2abc","name":"web","previous_name":"api"}}
```

`renamed.name` is the new name. `renamed.previous_name` is the name before the command, or `null`.

### `pin` and `unpin`

```json
{"schema_version":1,"pinned":{"id":"k3xq2abc","pinned":true}}
```

### `kill`

```json
{"schema_version":1,"killed":"k3xq2abc"}
```

`killed` is the identifier of the session that ended.

```json
{"schema_version":1,"killed_sessions":["k3xq2abc","m4ab5ghi"]}
{"schema_version":1,"would_kill":["k3xq2abc","m4ab5ghi"]}
```

`kill --all` gives `killed_sessions`: the identifiers of the sessions that ended. With `--dry-run`, `would_kill`: the identifiers of the sessions that would end.

### `send`

```json
{"schema_version":1,"sent":{"id":"k3xq2abc","bytes":11}}
```

`sent.bytes` is the number of bytes that the session got, with the Enter.

### `daemon status`

```json
{"schema_version":1,"daemon":{"running":true,"version":"0.1.0","pid":41870,"sessions":1,"configuration":"/home/user/.config/nomux/config.toml"}}
{"schema_version":1,"daemon":{"running":false,"configuration":"/home/user/.config/nomux/config.toml"}}
```

| Field | Type | Meaning |
|---|---|---|
| `daemon.running` | boolean | `true` if a daemon runs. If it is `false`, `daemon` has only `configuration` and the exit code is 2 |
| `daemon.version` | string | The product version of the daemon |
| `daemon.pid` | number | The process ID of the daemon |
| `daemon.sessions` | number | The number of sessions |
| `daemon.memory_limit` | object | `applied` (`true` if the operating system ends the processes of a session at its limit) and `detail` (how, or why not, in English). Not in the document from an older daemon |
| `daemon.sandbox` | object | What the host applies of a sandbox: `enforced` and `missing` (feature names) and `detail` (the kernel and its interface, in English). Not in the document from an older daemon |
| `daemon.configuration` | string | The path of the global configuration file of the local host, also if no daemon runs. Not in the document with `--host`, in the `daemon` object of `host add`, or if `HOME` is not set |

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
| `doctor[].checks[].name` | string | `connection`, `platform`, `binaries`, `permissions`, `daemon`, `versions`, `memory-limit`, `free-space` or `shared-build-output` |
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

### `project` and `group`

A change:

```json
{"schema_version":1,"change":{"type":"project_added","revision":3,"node":{"id":"alpwrus76mfj","parent_id":"qyfketjcadky","name":"acme-web","kind":"project","path":"Client work/Acme/acme-web","directory":"/home/user/src/acme-web"}}}
```

A listing (`project ls`, `group ls`, `group ls --tree`):

```json
{"schema_version":1,"workspace":{"revision":7,"nodes":[{"id":"qyfketjcadky","parent_id":null,"name":"Acme","kind":"group","path":"Acme"},{"id":"alpwrus76mfj","parent_id":"qyfketjcadky","name":"acme-web","kind":"project","path":"Acme/acme-web","directory":"/home/user/src/acme-web"}]}}
```

| Field | Type | Meaning |
|---|---|---|
| `change.type` | string | `project_added`, `project_removed`, `group_created`, `group_removed`, `renamed` or `moved` |
| `change.revision`, `workspace.revision` | number | The revision of the tree after the change, or of the listed tree |
| `change.node` | node | The node after the change. A removed node has its last name and path |
| `workspace.nodes[]` | list of nodes | The Projects (`project ls`), the Groups (`group ls`) or each node (`group ls --tree`), in the sequence of a depth-first walk |

A node has these fields:

| Field | Type | Meaning |
|---|---|---|
| `id` | string | The identifier |
| `parent_id` | string or `null` | The Group of the node. `null` at the top level |
| `name` | string | The name |
| `kind` | string | `group` or `project` |
| `path` | string | The names from the top level down to the node, with `/` between them. Not in an event |
| `directory` | string | Only for a Project: its directory on the host |

### `worktree ls`

```json
{"schema_version":1,"worktrees":{"projects":[{"id":"w7mdj3j7icns","path":"acme","unavailable_reason":null,"base_branch":"origin/main","last_fetch_at":1795707021434,"free_bytes":33204289536,"file_system_bytes":404729364480,"worktrees":[{"path":"/home/user/src/acme-auth","branch":"auth-flow","head":"9df43faf1fc4b1f8a7941e3ef8b47c72d820b5bf","is_main":false,"is_bare":false,"state":"finished","flags":["upstream-gone"],"sessions":[],"size":{"total_bytes":3026944,"build_output_bytes":3006464,"measured_at":1795707035291,"is_old":false,"shared_extents":"unknown","build_outputs":[{"path":"target","rule":"cargo-target","bytes":3006464}]},"last_activity_at":1795707021327,"merged_by":"ancestor","unpushed_commits":0,"removable":true,"refusals":[],"ignored_user_paths":[],"locked_reason":null,"facts_unavailable_reason":null}]}]}}
```

| Field | Type | Meaning |
|---|---|---|
| `worktrees.projects[]` | array | One object for each listed Project, in the sequence of the workspace tree |
| `projects[].id`, `projects[].path` | string | The identifier and the path of the Project in the workspace tree |
| `projects[].unavailable_reason` | string or `null` | Why the Project has no worktrees, and what to do. `null` when it has them |
| `projects[].base_branch` | string or `null` | The branch of the merge rules |
| `projects[].last_fetch_at` | number or `null` | The last fetch: milliseconds after the Unix epoch. `null`: never |
| `projects[].free_bytes`, `projects[].file_system_bytes` | number or `null` | The free space for a user without special rights and the size of the file system of the Project directory |
| `worktrees[].path` | string | The directory of the worktree |
| `worktrees[].branch`, `worktrees[].head` | string or `null` | The branch (`null` for a detached `HEAD`) and the commit |
| `worktrees[].is_main`, `worktrees[].is_bare` | boolean | The main worktree; the entry of a bare repository |
| `worktrees[].state` | string | `active`, `finished`, `stale`, `idle` or `unknown` |
| `worktrees[].flags` | array of strings | `dirty`, `unpushed`, `detached-commits`, `locked`, `submodule`, `ignored-user-files`, `busy`, `missing`, `other-file-system`, `upstream-gone` |
| `worktrees[].sessions` | array of strings | The identifiers of the sessions in the worktree |
| `worktrees[].size` | object or `null` | `null` when not measured yet. `total_bytes`, `build_output_bytes`, `measured_at` (milliseconds after the Unix epoch), `is_old`, `shared_extents` (always `unknown`: blocks shared by reflinks count in full), `build_outputs[]` (`path`, `rule`, `bytes`, the largest first) |
| `worktrees[].last_activity_at` | number or `null` | The last activity: milliseconds after the Unix epoch |
| `worktrees[].merged_by` | string or `null` | `ancestor`, `tree` or `upstream-gone`. `null`: not merged |
| `worktrees[].unpushed_commits` | number | The commits that are not pushed. The count stops at 1000 |
| `worktrees[].removable` | boolean | `true` when `refusals` is empty |
| `worktrees[].refusals` | array of strings | Why a removal of the directory is refused, the most important first: `main-worktree`, `directory-missing`, `facts-unknown`, `active-session`, `process-running`, `uncommitted-changes`, `unpushed-commits`, `detached-commits`, `locked`, `submodule`, `ignored-user-files`, `other-file-system`, `not-finished` |
| `worktrees[].ignored_user_paths` | array of strings | The ignored paths that are not build output, 32 at most |
| `worktrees[].locked_reason` | string or `null` | The reason of the lock |
| `worktrees[].facts_unavailable_reason` | string or `null` | Why the git facts of this worktree are not known |
| `worktrees[].tool` | string or `null` | The agent tool that probably made the worktree |
| `worktrees[].mark` | string or `null` | `done` or `keep` |
| `worktrees[].trash_id` | string or `null` | The identifier of the trash entry when the worktree is in the trash place |

The refusals also have `kept`, `no-trash-place` and `in-trash`.

### `worktree clean`

```json
{"schema_version":1,"plan":{"id":"9f0c2a1b7d3e4f50","total_bytes":2936832,"grace_ms":86400000,"items":[{"project_id":"z4idbpqpfmpl","worktree":"/home/user/src/acme-done","step":"build-output","path":"/home/user/src/acme-done/target","rule":"cargo-target","bytes":2936832,"branch":"done","head":"5c0c3883751b9243aa7a9a0d699a3b1fb551b41e","runs":true,"refusals":[],"warnings":[],"deletes_branch":false,"branch_kept_reason":null,"ignored_user_paths":[]}]}}
```

With `--dry-run`, and when no item runs, the document has `plan`. After a clean it has `clean`: `plan_id`, `planned_bytes`, `freed_bytes` and `results[]`, each with the fields of an item and `outcome` (`done`, `changed`, `refused`, `failed`) and `message`.

| Field | Type | Meaning |
|---|---|---|
| `plan.id` | string | The identifier of the plan |
| `plan.total_bytes` | number | The bytes of the items that run: an upper bound |
| `plan.grace_ms` | number | The grace time of the trash place |
| `items[].worktree` | string | The directory of the worktree |
| `items[].step` | string | `build-output`, `trash`, `delete`, `prune` or `repair` |
| `items[].path` | string or `null` | The build output directory, the path in the trash place, or the directory of a repair proposal |
| `items[].rule` | string or `null` | The build-output rule |
| `items[].bytes` | number | The allocated bytes that the step frees |
| `items[].runs` | boolean | `true` when `refusals` is empty |
| `items[].refusals` | array of strings | As in `worktree ls` |
| `items[].warnings` | array of strings | Facts to read before a clean |
| `items[].deletes_branch` | boolean | The end of the grace time also deletes the branch |
| `items[].branch_kept_reason` | string or `null` | Why the branch stays with `--delete-branch` |
| `items[].ignored_user_paths` | array of strings | The ignored files that are not build output |

### `worktree trash ls`, `worktree restore`, `worktree trash empty`, `worktree mark`

`worktree trash ls` has `trash`, a list of entries: `id`, `project_id`, `path`, `trash_path`, `branch`, `head`, `trashed_at`, `expires_at` (milliseconds after the Unix epoch) and `deletes_branch`. `worktree restore` has `restored`, one entry. `worktree trash empty` has `trash_emptied`, a list of entries with `removed`, `message`, `branch_deleted` and `branch_kept_reason`. `worktree mark` and `unmark` have `marked` with `path` and `mark` (`done`, `keep` or `null`).

### `status`

```json
{"schema_version":1,"status":{"attention":{"session":"k3xq2abc","status":{"state":"needs_you","kind":"permission","source":"program_status","guess":false,"since_ms":1795670000000,"seen":false,"message":{"text":"run tests?","source":"program_status"},"title":null,"progress":null,"tool":null}},"sessions":[{"id":"k3xq2abc","name":null,"command":["claude"],"status":{...},"aggregate":{...}}]}}
```

| Field | Type | Meaning |
|---|---|---|
| `status.attention` | object or `null` | The session with the most urgent status, and that status. `null` if the host has no session |
| `status.sessions[]` | list | One Agent run for each session that is not ended and for each ended session that keeps its last Agent run, in the sequence of the identifiers |
| `.id`, `.name`, `.command` | string, string or `null`, list of strings | The session, its name and its command |
| `.status` | status | The status of the Agent run |
| `.aggregate` | status | The most urgent status of the Agent run and of its Child work |
| `.children[]` | list | Only with `--tree`. Each Child work: `name`, `status` (a status, or `null` if no source reports it), `aggregate` and `children` |
| `.details` | object | Only if an adapter gave them: `model`, `provider_session_id` and `resume_command`, each a string or `null` |
| `status.workspace[]` | list | Only with `--tree`. Each node of the workspace tree, with the fields of a node of `project ls`, and `aggregate`: the most urgent status of the sessions below it, or `null`; and `sessions`: the identifiers of the sessions of a Project, empty for a Group |

A status has these fields:

| Field | Type | Meaning |
|---|---|---|
| `state` | string | `idle`, `working`, `done`, `needs_you`, `error`, or `unknown` for a state that the CLI does not know |
| `kind` | string or `null` | For `needs_you`: `permission`, `question` or `auth`, or `null` if no source says it |
| `source` | string | `adapter`, `program_status`, `self_report`, `session_restore`, `prompt_marks`, `foreground_program`, `none` (no source reports the work) or `unknown` |
| `guess` | boolean | `true` if the daemon guessed the state: the source is `prompt_marks` or `foreground_program` |
| `since_ms` | number | When the daemon saw the state for the first time: milliseconds after the Unix epoch |
| `seen` | boolean | For `done` and `error`: `true` if the user saw the result |
| `message`, `title`, `tool` | object or `null` | `{"text": ..., "source": ...}`: the text and its own source |
| `progress` | object or `null` | `{"percent": ..., "source": ...}` |

### `status set`

```json
{"schema_version":1,"status_set":{"session":"k3xq2abc","state":"working"}}
```

### `agent explain`

```json
{"schema_version":1,"explain":{"session":"k3xq2abc","run":{"id":"k3xq2abc","status":{...},"aggregate":{...},"children":[]},"sources":[{"source":"program_status","guess":false,"reported":true,"state":"working","kind":null,"message":"reading files","note":"1 program status records in the terminal stream"}]}}
```

`explain.run` has the fields of an entry of `status.sessions` with its Child work, without `name` and `command`. `explain.sources` has each source in the sequence in which they decide. `reported` is `false` for a source that says nothing: then `state`, `kind` and `message` are `null`, and `note` says why. The text of a note is not stable.

### `events --follow`

Each line is one document:

```json
{"schema_version":1,"event":{"seq":12,"at_ms":1795670000000,"type":"agent_run_changed","session":"k3xq2abc","run":{...}}}
{"schema_version":1,"event":{"seq":13,"at_ms":1795670004000,"type":"agent_run_removed","session":"k3xq2abc"}}
{"schema_version":1,"event":{"seq":13,"at_ms":1795670005000,"type":"snapshot"}}
```

| Field | Type | Meaning |
|---|---|---|
| `event.seq` | number | The position of the event in the event log of the daemon. The numbers start again when the daemon starts again |
| `event.at_ms` | number | When the event occurred: milliseconds after the Unix epoch |
| `event.type` | string | `agent_run_changed`, `agent_run_removed`, `extension_event`, `workspace_changed`, `session_hibernated`, `session_woken`, `session_wake_failed`, `session_memory_limit_reached`, `snapshot`, or `unknown` for an event that the CLI does not know |
| `event.session` | string | The session. Only in `agent_run_changed`, `agent_run_removed` and the four session events |
| `event.run` | object | Only in `agent_run_changed`: the Agent run after the change, with the fields of `explain.run` |
| `event.extension` | object | Only in `extension_event`: `id` (the extension), `name` (the name of the event) and `payload_base64` (the payload in base64: the extension selects its encoding) |
| `event.workspace` | object | Only in `workspace_changed`: `revision`, and `change`: `added` with `node` (a node without `path`) and `index`, `renamed` with `id` and `name`, `moved` with `id`, `parent_id` and `index`, `removed` with `id`, or `unknown` |

| `event.hibernation` | object | Only in the four session events, with the fields of the event: `reason`, `freed_bytes` and `measure`, and the numbers of the trigger (`live_agents` and `max_live_agents`, `memory_bytes` and `memory_limit_bytes`, or `pressure_stall_ms` and `pressure_window_ms`) for `session_hibernated`; `resume_command` for `session_woken`; `resume_command` and `error` for `session_wake_failed`; `limit_bytes` for `session_memory_limit_reached` |

After a `snapshot`, one `agent_run_changed` event for each session gives the state of the host now. A reader discards the state that it had.

### `hook`

The command prints the answer for the tool, not a JSON document, also with `--json`.

### `hooks install` and `hooks uninstall`

```json
{"schema_version":1,"hooks_change":{"action":"install","tool":"testtool","path":"/home/user/.testtool/settings.json","diff":"--- ...","changed":true,"applied":false,"restores_original":false}}
```

| Field | Type | Meaning |
|---|---|---|
| `hooks_change.action` | string | `install` or `uninstall` |
| `hooks_change.path` | string | The settings file |
| `hooks_change.diff` | string | The change as a unified diff. Empty if nothing changes |
| `hooks_change.changed` | boolean | `true` if the diff is not empty |
| `hooks_change.applied` | boolean | `true` with `--apply`: the command wrote the change |
| `hooks_change.restores_original` | boolean | For `uninstall`: `true` if the file gets its bytes from before the install, `false` if only the entries of nomux go away |

### `hooks status`

```json
{"schema_version":1,"hooks":[{"tool":"testtool","adapter":"nomux.test-adapter","state":"installed","path":"/home/user/.testtool/settings.json","recorded":true,"problem":null}]}
```

| Field | Type | Meaning |
|---|---|---|
| `hooks[].tool`, `.adapter` | string, string or `null` | The tool and its adapter extension. `null`: the tool has no adapter, only a record of an install |
| `.state` | string or `null` | `installed`, `missing` or `different`. `null` if the state is not known: `problem` says why |
| `.path` | string or `null` | The settings file |
| `.recorded` | boolean | `true` if nomux has a record of an install: `hooks uninstall` can remove the hooks |

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
| `error.candidates` | list | Only for a target that can be more than one session: each candidate as `id`, `name` (string or `null`) and `state` |

```json
{"schema_version":1,"error":{"code":1,"kind":"error","message":"ap can be more than one session. ...","candidates":[{"id":"okjzhnzv","name":"api-v2","state":"running"},{"id":"ouldrfi3","name":"api","state":"running"}]}}
```

A wrong command line is not a JSON document: the argument parser writes its usage message as text.

## Tests

- `crates/nomux-cli/src/output/json.rs` and `crates/nomux-cli/src/output/extension_json.rs` have the full text of each document. A changed field fails these tests.
- `crates/nomux-cli/src/arguments.rs` has the rule for the arguments after `--` and `--cmd`, and the usage error for the two forms in one command line. `crates/nomux-cli/src/output/memory_human.rs` has the full text of the table of `ls --memory`, `crates/nomux-cli/src/output/status_human.rs` has the table of `status`, and `tests/session_memory.rs` has its JSON document with real processes. `crates/nomux-cli/src/attach` has the key decoder with the legacy bytes, the kitty form, `modifyOtherKeys`, a sequence in two reads and a paste, and the mode of the terminal, also after a panic. `crates/nomux-core/src/configuration/prefix_keys.rs` has the rules of a prefix key.
- `tests/` has the end-to-end tests that run the real `nomux`, `nomuxd` and `nomux-holder` and examine the exit codes. `tests/attach.rs` runs `nomux attach` in a pseudoterminal and examines the screen. `tests/remote_host.rs` runs the `host` commands and the commands with `--host` against an SSH server that the test starts. `tests/extensions.rs` runs the `ext` commands with a test extension that uses the extension SDK. `tests/restore.rs` stops each process as a restart of the host does and runs `ls`, `attach`, `start` and `restore-policy` on the dormant sessions. `tests/journal.rs` has `capture`, and `tests/daemon_service.rs` has the service file. `tests/status.rs` has `status`, `status set`, `agent explain` and `events --follow` with a program that reports its status. `tests/adapters.rs` has `hook`, the `hooks` commands and the event of an extension with the test adapter extension. `tests/session_names.rs` has a name after a restart of the host and two `new -A` at the same time. `tests/session_scripting.rs` has `send`, `kill --all` with its question, `attach -d`, `attach -r` and bare `nomux`. `crates/nomux-client/src/session_target.rs` has the table of the target rule. `tests/workspace.rs` has the `project` and `group` commands, the tree after a restart of the daemon, two clients that keep the same tree with the events, and `status --tree` with Groups. `tests/hibernation.rs` has `hibernate`, `wake`, `ls` and `capture` of a hibernated session, and `attach` as a wake.
- `crates/nomux-cli/src/service_files` has the content of the systemd unit and of the launchd agent, also for a path that needs quotes.

## Open points for the next versions

- `detach` without an identifier in a session.
- The scrollback in an attach: an attach shows the screen of the session and not the lines that left it. `capture` prints them with their escape sequences: it does not make the text of a screen from them.
- A command that removes an ended session before its retention time is over, and a command that starts the command of an ended session again.
- A restore policy for a new session as an option of `new`.
- The other commands of the command reference, also `ext update` and `ext pack`, and the installation of an extension from a registry.
- `host doctor` for the local host, and a check of the agent hooks.
- `hooks install` for a remote host, and settings files that are not JSON.
- The context from `NOMUX_*` variables, other than for `status set`.
- `wait` for a status, and `events` without `--follow`.
