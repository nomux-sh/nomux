# Sandbox of a session

This document describes the sandbox of a session: what it is, the built-in profiles and their rules, what it protects against and what it does not, how to write a profile, and how to see that a session is sandboxed. This version is the first step of issue [#109](https://github.com/nomux-sh/nomux/issues/109): Linux, file rules, and the network on or off.

[holder.md](holder.md) describes the holder, which applies the limits. [daemon.md](daemon.md) describes the daemon, which resolves a profile. [cli.md](cli.md) has the commands. The control protocol has the section "Sandbox" in `spec/control.md` of the [`proto`](https://github.com/nomux-sh/proto) repository.

## What it is

A sandbox limits what the processes of a session can do: which files they can read, run and change, and if they can use the network. The user selects a sandbox for each session. A session without a profile has no sandbox.

```sh
nomux new --sandbox project -- claude
```

- **The holder applies the limits to the command, before the command starts.** The holder reads the plan of the daemon and builds the limits in its own process. Then its child, between `fork` and `exec`, applies them to itself with three system calls: `prctl(PR_SET_NO_NEW_PRIVS)`, `landlock_restrict_self` and, for a profile without network, `prctl(PR_SET_SECCOMP)`. The child allocates no memory there: each value is ready before the `fork`. The holder itself has no limit: it writes the journal and owns the sockets.
- **No process is added, and the command line of the tool does not change.** The limits pass to each process that the command starts, and no process can remove them.
- **With a memory limit**, the holder starts the command through `systemd-run`, which needs the bus of the user manager. A sandbox must not reach that bus: with it, a process can start a unit outside the sandbox. Thus the launcher starts `nomux-holder --enter-sandbox <plan> -- <command>`: the same binary applies the same plan to itself and starts the command in its place with `exec`. It is one more `exec`, not one more process.
- **The plan is resolved at each start.** A new session, a restore and a wake resolve the profile again, with the configuration and the home directory of that time. The daemon writes the plan to a file of mode 0600 in the runtime directory. The holder reads it and removes it before the command starts: the command cannot read it.
- **A sandboxed session takes a new holder, not a pooled one.** A pooled shell runs before its session exists, and a sandbox cannot limit a process that runs.
- **Fail closed.** If the host cannot apply a feature that the profile requires, the session does not start, and the reason names the missing features. A restore or a wake that fails this way leaves the session dormant with the reason. A holder that is older than the sandbox does not know `--sandbox-plan`: it ends with its usage error and starts no command, and the session does not start.

On Linux the limits are [Landlock](https://docs.kernel.org/userspace-api/landlock.html) (files, Unix sockets, signals, abstract sockets), `no_new_privs` (no privilege gain through a setuid program: `sudo` does not work in a sandbox) and a seccomp program. On a different operating system, the host applies no feature, and a session with a profile other than `off` does not start. macOS is step 2 of the issue.

## Features

| Feature | Meaning | Linux |
|---|---|---|
| `files` | The processes can read, run and change files only where the profile permits it | Landlock interface 1 (kernel 5.13) |
| `network_off` | The processes cannot open a socket of an internet protocol | seccomp: `socket` fails for `AF_INET`, `AF_INET6`, `AF_PACKET` and `AF_VSOCK`, and `io_uring_setup` fails. With Landlock interface 4, TCP `bind` and `connect` are refused too |
| `unix_sockets` | The processes can connect to a Unix socket only below a path that the profile permits | Landlock interface 9 (kernel 7.1) |
| `signals` | The processes cannot send a signal to a process outside the sandbox | Landlock interface 6 (kernel 6.12) |
| `abstract_sockets` | The processes cannot connect to an abstract Unix socket of a process outside the sandbox | Landlock interface 6 |

A profile **requires** `files`, and `network_off` if it has no network: without them the session does not start. It **wants** `unix_sockets`, `signals` and `abstract_sockets`: on a kernel without them, the session starts in the state `partial`, and `nomux ls` names what is missing. With `strict = true` in `[sandbox]`, a profile requires them too, and the partial state does not occur.

The daemon asks the kernel with the system calls, not with the version of the kernel. Thus a container whose policy blocks Landlock counts as a host without it.

## Built-in profiles

| Profile | Read and run | Change | Network |
|---|---|---|---|
| `off` | each file | each file | open (no sandbox) |
| `project` | the system paths, the home directory without the secret paths | the project directory, the temporary directory of the session, `/dev`, and the state of the agent tool | open |
| `project-offline` | the same | the same | off |
| `read-only` | the same, and the project directory | the temporary directory of the session and `/dev` | off |

**The system paths** are `/usr`, `/bin`, `/sbin`, `/lib`, `/lib32`, `/lib64`, `/opt`, `/etc`, `/sys`, `/proc`, `/var/lib` and `/run/systemd/resolve`. A path that does not exist adds no rule. `/dev` can be changed: the terminal, `/dev/null` and `/dev/shm`. The directory of the binaries of the daemon can be read, thus `nomux status set` and `nomux hook` work in the session.

**Not permitted:** the home directories of other users, `/root`, `/mnt`, `/media`, `/srv`, `/run/user/<uid>` (the sockets of the user), `/var/run`, `/tmp` and `/var/tmp`.

**The project directory** is the closest directory at or above the working directory with a `.git`, or the working directory. The search stops below the home directory. A sandbox refuses a project directory that is the home directory or above it, or that is in a secret path: the command could then read the secret paths. In a **git worktree**, the command can also change `objects`, `refs` and `logs` of the common git directory and the directory of the worktree there, and read the rest of the common git directory: a commit works, and a change of `hooks` or `config` of the repository is refused.

**The temporary directory of the session** is `tmp` in the session state directory. `TMPDIR` of the command is that directory. `/tmp` is not permitted, because it has the sockets and the files of other programs. A tool that writes to `/tmp` without `TMPDIR` fails.

**The secret paths** below the home directory are never readable. A path that does not exist adds no rule.

| Group | Paths |
|---|---|
| Keys | `.ssh`, `.gnupg`, `.password-store`, `.local/share/keyrings`, `.config/1Password`, `.config/Bitwarden` |
| Cloud and CI | `.aws`, `.azure`, `.config/gcloud`, `.kube`, `.docker`, `.config/gh`, `.config/glab-cli`, `.netrc`, `.git-credentials`, `.terraform.d`, `.config/doctl`, `.cloudflared`, `.config/helm` |
| Package registries | `.npmrc`, `.pypirc`, `.cargo/credentials.toml`, `.cargo/credentials`, `.gem/credentials`, `.m2/settings.xml` |
| Browsers | `.mozilla`, `.config/google-chrome`, `.config/chromium`, `.config/BraveSoftware`, `.config/microsoft-edge`, `.var` |
| Agent tools | `.claude`, `.claude.json`, `.codex`, `.gemini`, `.config/opencode`, `.cursor`, `.copilot`, `.config/github-copilot`, `.grok` |
| nomux | `.nomux/run`, `.nomux/downloads`, `.local/state/nomux`, `.local/share/nomux`, `.config/nomux` |
| History | `.bash_history`, `.zsh_history`, `.local/share/fish/fish_history`, `.python_history`, `.node_repl_history`, `.psql_history`, `.mysql_history`, `.lesshst` |

Landlock permits paths and cannot take a part of a permitted directory away. Thus the daemon lists the home directory and gives one rule for each entry that is not secret, and for a secret path with more parts, such as `.config/gh`, it goes into `.config`. **An entry that is made after the start of the session cannot be read** by the command: it has no rule. A symbolic link in the home directory into a secret path gets no rule.

**The state of the agent tool.** If the command of the session is a known agent tool, its state leaves the secret list and can be changed. This version knows one tool:

| Program | State |
|---|---|
| `claude` | `.claude`, `.claude.json` |

The rule applies only when the tool is the command of the session, not when a shell of the session starts it later: a sandbox cannot change after the start.

**The environment.** The daemon passes its environment, as for each session ([daemon.md](daemon.md#environment-of-a-session)), with three changes: `NOMUX_SOCK` is the agent socket (see "Socket guard"), `TMPDIR` is the temporary directory of the session, and `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, `DBUS_SESSION_BUS_ADDRESS` and `DOCKER_HOST` are removed. A secret in another variable of the daemon reaches the command.

## Socket guard

A process of a sandboxed session is a process of the same user. Without a guard, it could connect to the socket of the daemon and type a command into a session without a sandbox.

- **The agent socket.** The daemon has a second socket, `nomuxd.agent/sock` in the runtime directory. It takes the handshake, `Agent.SetStatus` and `Hooks.Event` only: the two requests with which a session reports about itself, which check the token of the session. Each other request gets `ERROR_CODE_PERMISSION_DENIED`. `NOMUX_SOCK` of a sandboxed session is this socket, thus `nomux status set` and `nomux hook` work.
- **With `unix_sockets` (Linux 7.1 and later)**, the sandbox can connect only to the sockets below `nomuxd.agent`: the main socket of the daemon, the sockets of the holders, the bus of the user and the agent of SSH are refused by the kernel.
- **Without `unix_sockets`**, the connect works. The daemon then reads the process ID of each peer of its main socket. If the process descends from the command of a sandboxed session, the daemon answers its first request with `ERROR_CODE_PERMISSION_DENIED` and closes the connection: the CLI ends with the exit code 4. A process that left the tree escapes this check: for example a process whose parent ended, which the system gave to its first process. The sockets of the holders stay reachable. **"A sandboxed session cannot control other sessions" is a guarantee only with `unix_sockets`.**

## What it protects against

1. A wrong command that deletes or changes files outside the project.
2. An instruction in a file that makes the agent read the keys, the cloud tokens, the browser data or the tokens of other agent tools.
3. The same instruction writing outside the project: the start files of the shell, a program in a `PATH` directory, other repositories.
4. Simple sending of data when the network is off.
5. A process of the session that controls other sessions or the daemon: with `unix_sockets`, or with the check of the daemon above.
6. Signals to processes outside the sandbox, with `signals`.

## What it does not protect against

- A defect of the kernel, of Landlock or of seccomp.
- An action that the user permitted: the project directory can be changed by design, thus the agent can destroy the project. Use git.
- Data that leaves through the network when it is open, or through a host that it can reach.
- A program that the user runs outside the session, for example a `git commit` that runs a hook that the agent wrote in a main checkout.
- The names, sizes and times of the files of a secret path: `stat` works, only the content is refused. The command lines of other processes in `/proc`. Secrets in the environment of the daemon.
- The sandbox does not replace the permission questions of an agent tool.

## What this version cannot do

- Filter the network by host name: the network is open or off. A list of hosts comes in step 2, with a small proxy helper.
- Report a denied access as an event: the command sees its own error, for example `Permission denied`. The audit records of the kernel come in step 3.
- Protect `.git/hooks` and `.git/config` in a main checkout: `.git` is in the project directory, which the command can change. In a git worktree they are protected.
- Hide the names and the sizes of files.
- Run on macOS (step 2) or Windows.
- Read the needs of a tool from its adapter: the table above is in the daemon. The manifest of an adapter gets a field for it in a later version of the extension API.

## How to write a profile

A profile of the user extends a built-in profile. It is in the configuration file, `[sandbox.profiles.<name>]`:

```toml
[sandbox]
default = "off"                 # the profile of a session that names none
strict = false                  # true: a host without a wanted feature does not start a session

[sandbox.tools]                 # the default for a command that is this program
claude = "project"

[sandbox.projects]              # the default for a working directory at or below this directory
"/home/user/src/api" = "project-offline"

[sandbox.profiles.rust]
extends = "project"
read = ["~/.rustup", "~/.cargo/bin"]
write = ["~/.cargo/registry", "~/.cargo/git"]
deny_read = ["~/work/private"]
network = "open"                # open | off
```

| Key | Meaning |
|---|---|
| `extends` | The built-in profile: `project`, `project-offline` or `read-only`. Necessary |
| `read` | More paths that the command can read and run. An absolute path, or `~/` and a path |
| `write` | More paths that the command can change |
| `deny_read` | More secret paths below the home directory |
| `network` | `open` or `off`. Without it, the network of the extended profile |

- A name has 1 to 64 characters from `a-z`, `0-9` and `-`.
- **Precedence** of the profile of a new session: `nomux new --sandbox`, then the longest directory of `[sandbox.projects]` at or above the working directory, then `[sandbox.tools]` for the program of the command, then `sandbox.default`. `--sandbox off` is no sandbox, also when a default names a profile.
- A secret path is never permitted, also not by `read` or `write`. A secret path that exists below a permitted path is an error: a rule cannot take a part of a directory away.
- The daemon reads the file at its start. A wrong value names its key in the log of the daemon, and the daemon then uses its defaults for the whole file: [daemon.md](daemon.md#configuration). **Thus a mistake in the file turns the defaults of `[sandbox.tools]` and `[sandbox.projects]` off.** A session that names its profile with `--sandbox` never starts without it: a profile that the daemon does not know is an error.
- A remote host resolves the profile with its own configuration file and its own home directory: the client sends only the name. A client sends a profile only to a daemon with the capability `session_sandbox`. An older daemon would ignore the name and start the session without limits, thus `nomux new --sandbox` fails there.

## How to see that a session is sandboxed

```console
$ nomux ls
ID        NAME  STATE    PID     SANDBOX   COMMAND  NOTE
k3xq2abc  api   running  41873   project   claude
m4ab5ghi  -     running  41901   project*  sh       sandbox without unix_sockets
p2rs6tuv  -     running  41950   -         vim
```

- The column `SANDBOX` has the profile, and `-` for a session without a sandbox. `*` marks the state `partial`, and the note names the missing features. The table has the column only if a session has a sandbox. On a narrow terminal, the table leaves out the column after `PID` and `PROJECT`.
- `nomux ls --json` and `nomux new --json` have `session.sandbox`: `profile`, `state` (`enforced`, `partial`, `inactive` for a session without a process, or `unknown`), `enforced` and `missing`.
- `nomux host doctor` has the line `sandbox` with the kernel, the Landlock interface and the features. The line always passes; on a host that applies no file rule, it says that a session with a sandbox profile does not start there. `nomux daemon status` has the same text.

```console
$ nomux host doctor
local
  ok    daemon   nomuxd 0.5.0 runs: process ID 41870, 2 sessions
  ok    sandbox  linux 7.1.7, landlock ABI 9: files, network_off, unix_sockets, signals, abstract_sockets
```

## Measurements

Measured on Linux x86-64 (kernel 7.1.7, Landlock interface 9) with the release profile.

| Measurement | Result | Target |
|---|---|---|
| Size of `nomux-holder` (musl, static), without and with the sandbox | 758448 bytes, 795312 bytes (+36864) | < 1 MB |
| Resident memory of a holder (musl, static) of `sleep`, without and with `--sandbox project` | 964 kB, 988 kB | < 3 MB |
| `nomux new --sandbox project`, `bash` with an empty start file: median, slowest of 50 | 3.65 ms to 3.82 ms, 4.32 ms to 5.12 ms | < 10 ms |
| The same, request to the first command of the shell | 3.03 ms to 3.15 ms, 3.37 ms to 3.52 ms | < 10 ms |
| `nomux new` without a sandbox and without the pool, before and after this change, in one run of the same computer | 3.41 ms, 3.35 ms | no change |

A sandboxed session takes a new holder, thus it compares with the row "without the pool" of [daemon.md](daemon.md#measurements) (2.7 ms to the first command), not with the pool (1.1 ms to 2.3 ms). The time of the sandbox itself is in the start of the holder: it reads the plan, opens each path and makes the rule set. The rows are from the program `tests/measurements/new_session.rs` with the release build: [tests/README.md](../tests/README.md) says how to run it. Each range is the result of 2 runs. The home directory of the measurement has a few entries; each entry of a home directory adds one rule, approximately 3.5 us.

## Unsafe code

The sandbox has its `unsafe` code in one file, `crates/nomux-sandbox/src/linux/system_calls.rs`, with a `SAFETY` comment for each block. Issue [#109](https://github.com/nomux-sh/nomux/issues/109) is the agreement for it. [holder.md](holder.md#unsafe-code) has the list.
