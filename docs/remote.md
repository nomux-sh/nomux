# Remote hosts

This document describes how nomux uses a remote host through SSH: the SSH client, the channels, the installation of the binaries on the host, the daemon on the host, and what an attach does when the connection ends.
The SSH client is in `crates/nomux-transport` (module `ssh`). The bootstrap, the two kinds of host and the reconnect are in `crates/nomux-client`. The commands are in `crates/nomux-cli`.

[cli.md](cli.md) has the `host` commands and the `--host` option. [daemon.md](daemon.md) describes the daemon, which is the same program on each host.

## Overview

```
nomux (CLI) --- one SSH connection (TCP) ----------------> sshd on the host
   |   channel: direct-streamlocal --- control envelopes --> ~/.nomux/run/nomuxd.sock --> nomuxd
   |   channel: direct-streamlocal --- terminal frames ----> ~/.nomux/run/nomuxd.sock --> nomuxd
   |   channel: session (exec)     --- sh -c '...' --------> probe, upload, start of the daemon
```

```sh
nomux host add deploy@vps.example.com     # installs nomux on the host and saves the host as "vps.example.com"
nomux --host vps.example.com new --attach
```

The host needs an SSH server and a POSIX shell. It needs no nomux package, no internet access and no `scp` or `sftp`.
The daemon on the host is the daemon of [daemon.md](daemon.md). It listens only on its Unix socket. It has no network port.

## SSH client

nomux has its own SSH client. It uses the [`russh`](https://crates.io/crates/russh) library, version 0.65, with the `ring` cryptography library. It does not start the `ssh` program.

- **Authentication.** nomux tries the keys of ssh-agent first, then the key files. A key file with a passphrase is not read: add it to ssh-agent (`ssh-add`). nomux has no password, stores no secret and asks no question.
- **Never requested.** nomux never asks the host for agent forwarding, for a terminal (PTY) of the SSH connection, for X11 forwarding or for a TCP port forward.
- **Not written to a log.** The client writes no key and no content of a session to a log.

The sequence of an authentication:

1. Each key of the agent. The agent is the socket of `IdentityAgent`, or of the `SSH_AUTH_SOCK` variable. With `IdentitiesOnly yes`, only the keys that an `IdentityFile` names.
2. Each key file of `IdentityFile`. Without `IdentityFile`: `~/.ssh/id_rsa`, `id_ecdsa`, `id_ecdsa_sk`, `id_ed25519` and `id_ed25519_sk`, if they exist.

An RSA key signs with SHA-512 or SHA-256, as the host supports. If the host accepts no key, the command ends with exit code 4 and a message that names each key that was not usable.

## `~/.ssh/config`

nomux reads `~/.ssh/config` with the rules of OpenSSH: the first value of an option wins, `Host` and `Match` select the lines, and an `Include` is read at its position.
nomux does not read `/etc/ssh/ssh_config`.

### Supported

| Option | Notes |
|---|---|
| `Host` | Patterns with `*`, `?` and `!` |
| `Match` | The criteria `all`, `host`, `originalhost`, `user` and `localuser`, also with `!` |
| `Include` | A path that is not absolute is relative to `~/.ssh`. The last part of the path can have `*` and `?` |
| `HostName` | With `%h` and `%%` |
| `User`, `Port` | The user and the port of the SSH destination come before these options |
| `IdentityFile` | More than one. `none` removes all of them |
| `IdentitiesOnly` | |
| `IdentityAgent` | A socket path, `SSH_AUTH_SOCK`, `$VARIABLE` or `none` |
| `ProxyJump` | A list of `[user@]host[:port]` or `ssh://` addresses, or `none`. Each jump host uses its own configuration, also its own `ProxyJump` |
| `UserKnownHostsFile`, `GlobalKnownHostsFile` | More than one file. `none` is no file |
| `StrictHostKeyChecking` | `yes`, `ask`, `accept-new`, `no`, `off`: see "Trust rules" |
| `HostKeyAlias` | |
| `ConnectTimeout` | The default is 30 seconds |
| `ServerAliveInterval`, `ServerAliveCountMax` | The defaults of nomux are 5 seconds and 3: see "Connection parameters" |

A path can start with `~`, and it can have the tokens `%%`, `%d`, `%h`, `%k`, `%n`, `%p`, `%r` and `%u`, and `${VARIABLE}`.

### Not supported: the command fails

If one of these options applies to the host, nomux does not connect. The message names the option, the file and the line.

| Option | Condition |
|---|---|
| `ProxyCommand` | Each value that is not `none` |
| `CertificateFile`, `PKCS11Provider`, `SecurityKeyProvider`, `KnownHostsCommand` | Each value that is not `none` |
| `CanonicalizeHostname` | Each value that is not `no` or `none` |
| `BindAddress`, `BindInterface` | Each value |
| `Match` with `exec`, `canonical`, `final`, `tagged`, `localnetwork` or a different criterion | Only if the criteria before it on the line are true |
| `Include` with a wildcard in a directory name | |
| A token that is not in the list above | For example `%C` |

Example of the message:

```
nomux: /home/ada/.ssh/config line 12 sets `ProxyCommand` for this host, and the SSH client of nomux does not support `ProxyCommand`. nomux cannot connect to this host with this configuration
```

A fallback to the `ssh` program for such a host is not written.

### Ignored

Each other option changes nothing that nomux does, and nomux ignores it. Examples:

- Options for features that nomux never uses: `ForwardAgent`, `ForwardX11`, `LocalForward`, `RemoteForward`, `DynamicForward`, `RequestTTY`, `RemoteCommand`, `SendEnv`, `SetEnv`.
- Options of the `ssh` program: `ControlMaster`, `ControlPath`, `ControlPersist`, `LogLevel`, `EscapeChar`, `AddKeysToAgent`, `UseKeychain`, `UpdateHostKeys`, `HashKnownHosts`.
- The algorithm lists `Ciphers`, `KexAlgorithms`, `MACs`, `HostKeyAlgorithms` and `PubkeyAcceptedAlgorithms`: nomux uses the algorithms of its SSH library. Thus a host that needs an old algorithm can refuse the connection.
- The authentication methods that nomux does not have: `PasswordAuthentication`, `KbdInteractiveAuthentication`, `GSSAPIAuthentication`, `HostbasedAuthentication`.

## Trust rules

nomux checks the key of the host against the files of known hosts, as OpenSSH does: `~/.ssh/known_hosts`, `~/.ssh/known_hosts2`, `/etc/ssh/ssh_known_hosts` and `/etc/ssh/ssh_known_hosts2`, or the files of the configuration.
The name of the host in the files is the host name, or `[host]:port` if the port is not 22. Hashed names (`|1|...`), patterns and `@revoked` lines work. `@cert-authority` lines are ignored: nomux does not use host certificates.

| The files have | `StrictHostKeyChecking yes` or `ask` (default) | `accept-new`, `no` or `off` |
|---|---|---|
| The key that the host sent | Connect | Connect |
| No key of that type for the host | **Error** | Add the key to the first file of the user, then connect |
| A different key of that type for the host | **Error** | **Error** |
| The key on a `@revoked` line | **Error** | **Error** |

- nomux asks no question. Thus `ask` is the same as `yes`.
- A changed key is an error with each value. With `no`, OpenSSH connects and permits no forwarding, and nomux needs a forwarding.
- nomux asks the host for a key of a type that the files already have for the host, as OpenSSH does. Thus a host that is known cannot get a new trust decision with a key of a different type.

**A key that is not known.** The message has the fingerprint and the OpenSSH command that adds the key:

```
nomux: the host key of vps.example.com is not known. The host sent the ssh-ed25519 key with the fingerprint SHA256:... If the fingerprint is correct, run `ssh -o HostKeyAlgorithms=ssh-ed25519 'deploy@vps.example.com' exit` and answer `yes`, then run nomux again
```

The command makes `ssh` show the key of the same type. Compare the two fingerprints with the fingerprint that the administrator of the host gives.

**A key that changed.**

```
nomux: THE HOST KEY OF vps.example.com CHANGED. /home/ada/.ssh/known_hosts line 7 has a different ssh-ed25519 key. The host sent the key with the fingerprint SHA256:... Someone can be between this computer and the host. nomux does not connect. If the host has a new key, remove the old one with `ssh-keygen -R 'vps.example.com' -f '/home/ada/.ssh/known_hosts'`
```

## Channels

### One channel for each connection to the daemon

On the local host, a client opens one Unix connection for the control requests and one Unix connection for each attach ([daemon.md](daemon.md)).
On a remote host, each of these connections is one SSH channel of the type `direct-streamlocal@openssh.com`: the SSH server connects the channel to `~/.nomux/run/nomuxd.sock`, or to the socket in the short link of a long home directory ([daemon.md](daemon.md#length-of-the-socket-paths)). The client computes the path from the home directory of the host. The daemon sees a local connection of the same user.

Thus the protocol on a channel is the protocol of the local socket, without a change: the handshake, the control envelopes, and after `Session.Attach` the terminal frames in the two directions.

| Connection | Channel | From the client | To the client |
|---|---|---|---|
| Control | `direct-streamlocal` | Requests | Responses |
| Attach | `direct-streamlocal`, one for each attach | `Input` frames | `ScreenSnapshot`, `Output`, `End` frames |
| Window size during an attach | `direct-streamlocal` (a control connection) | `Session.Resize` | Responses |
| Bootstrap | `session` with `exec` | The bytes of a binary | The output of a script |

### Why not a bridge program on the standard input and output

The second design was one remote command for each connection, for example `nomuxd --stdio`, with the protocol on its standard input and output.

| | Channel to the socket (selected) | Bridge command |
|---|---|---|
| Processes on the host | None | One process for each connection |
| Path of a key | `sshd` to the socket | `sshd`, a pipe, the bridge, the socket |
| Login shell of the user | Not used | Starts each connection. A start file that writes text (`echo` in `.bashrc`) damages the stream |
| Needs on the server | Streamlocal forwarding | The `exec` request |

A server refuses the selected design if `sshd_config` has `AllowStreamLocalForwarding no` (or `remote`) or `DisableForwarding yes`, or if the key in `authorized_keys` has `restrict` or `no-port-forwarding`. The defaults of OpenSSH permit it, and `AllowTcpForwarding no` does not change it. Servers that follow a hardening guide with `DisableForwarding yes` refuse it.
For such a server nomux fails with a message that names these options. The bridge command as a second way for these servers is not written.

OpenSSH gives the same answer when it refuses the channel and when nothing listens on the socket. nomux tells the two apart with a command on the host: if the process of the lock file runs and the socket exists, the server refuses the channel.

### Input and bulk output

All channels of a host use one TCP connection. These properties keep input fast when a session writes a flood of output:

1. **Different directions.** Input goes from the client to the host. In that direction a connection carries only keys, small requests and window adjustments. Bulk data goes only from the host to the client. A full direction does not stop the other direction.
2. **A window for each channel.** Each channel has its own receive window of 256 KiB. The host sends at most that number of bytes that the client did not take. Thus the output of a flood that is on the way to a client has a limit, and behind that limit the rule for a slow client of the daemon applies: the client gets the current screen and not each byte ([daemon.md](daemon.md)).
3. **The reader always reads.** The client reads terminal output in a task that is independent of the task that sends input. Each channel has a queue of 8 packets (32 KiB each) in the client. When the queue of one channel is full, the client stops reading the connection until the reader takes one packet. A key waits at most for the time in which the terminal shows one packet.
4. **No delay for small packets.** The TCP connection has `TCP_NODELAY`.

The CLI opens one SSH connection for each command, thus two attached terminals do not share a connection. A client that shows more than one session on one connection (the desktop client) gets property 3 for each of them: one slow session delays the other sessions for one packet at most.

### Connection parameters

| Parameter | Value |
|---|---|
| Receive window of a channel | 256 KiB |
| Largest packet | 32 KiB |
| Queue of a channel in the client | 8 packets |
| `TCP_NODELAY` | On |
| Question to a silent host (`ServerAliveInterval`) | After 5 seconds without data. `ServerAliveInterval 0` stops the questions |
| Questions without an answer before the connection closes (`ServerAliveCountMax`) | 3 |
| Compression | None |

OpenSSH sends no question by default. nomux does, because an attach must learn in a short time that its connection is dead.

## Bootstrap

`nomux host add` and each command with `--host` do these steps on the SSH connection:

1. **Probe.** One command gives the output of `uname -s` and `uname -m`, the home directory of the user, and whether `~/.nomux/bin/<label>` has the three binaries.
2. **Target.** The client finds the release target of the host:

   | `uname -s` | `uname -m` | Target |
   |---|---|---|
   | `Linux` | `x86_64`, `amd64` | `x86_64-unknown-linux-musl` |
   | `Linux` | `aarch64`, `arm64` | `aarch64-unknown-linux-musl` |
   | `Darwin` | `x86_64` | `x86_64-apple-darwin` |
   | `Darwin` | `arm64` | `aarch64-apple-darwin` |

   A different host is an error.
3. **Install**, only if the binaries are absent:
   1. The client downloads `nomux-<version>-<target>.tar.gz` and `SHA256SUMS` of the release of its own version, and it verifies the SHA-256 checksum of the archive. It uses `curl` and `tar` of the local computer, the programs that `install.sh` also needs.
   2. It sends each binary to `~/.nomux/bin/.<label>.<random>/` on the host with `cat > file` on an `exec` channel.
   3. It compares the SHA-256 checksum of each file on the host with its own. The host makes the checksums with `sha256sum` or `shasum`. A host that has none of them sends the files back, and the client makes the checksums.
   4. It runs the new `nomuxd --version` on the host. Binaries for a different processor fail here.
   5. It renames the directory to `~/.nomux/bin/<label>`. The rename is the one step that makes the installation exist: a failure before it leaves no installation, and the client removes the temporary directory.

The client does not start the daemon in the bootstrap. The first request starts it: see "Daemon on the host".

`<label>` is the version of the client, for example `0.1.0`. The client always installs its own version, and a host can have more than one version.

### What the host needs

`sh`, `uname`, `cat`, `mkdir`, `chmod`, `mv`, `rm`, `ls` and `id`: the POSIX shell and its standard programs. `sha256sum` or `shasum` is used if it exists.
The login shell of the user can be a different shell. Each command is `sh -c '<script>'`, and a script has no single quote, thus each login shell gives the script to `sh` without a change.

### Source of the binaries

| Variable on the client | Function |
|---|---|
| (none) | The release on GitHub: `https://github.com/nomux-sh/nomux/releases/download/v<version>/` |
| `NOMUX_RELEASE_DOWNLOAD_URL` | A different address of the releases, also a `file://` address. `install.sh` has the same variable |
| `NOMUX_BOOTSTRAP_BINARIES` | A local directory that has `nomux`, `nomuxd` and `nomux-holder` for the host. The client downloads nothing |

**A development build has no release.** The download then fails with this message:

```
nomux: nomux 0.2.0 has no release archive for x86_64-unknown-linux-musl at https://.../nomux-0.2.0-x86_64-unknown-linux-musl.tar.gz. A development build has no release: build nomux, nomuxd and nomux-holder for x86_64-unknown-linux-musl and set the NOMUX_BOOTSTRAP_BINARIES variable to the directory that has them
```

Build the binaries for the target of the host and give the directory:

```sh
cargo build --release --target x86_64-unknown-linux-musl -p nomux-cli -p nomuxd -p nomux-holder
NOMUX_BOOTSTRAP_BINARIES=target/x86_64-unknown-linux-musl/release nomux host add deploy@vps.example.com
```

With `NOMUX_BOOTSTRAP_BINARIES`, the label is the version and the first 12 characters of a SHA-256 checksum of the binaries, for example `0.2.0-3f9a1c0b7e42`. Thus each build gets its own directory, and a binary that runs is never replaced. Set the variable for each command of that build.

## Daemon on the host

A request needs the daemon. The client opens a channel to the socket and does the handshake.

| State of the host | What the client does |
|---|---|
| A daemon answers, and the two have a common protocol version | Uses it. The daemon can be older or newer than the client: the handshake has the newest and the oldest protocol version of each side |
| No daemon listens | Runs `~/.nomux/bin/<label>/nomuxd --detach` and connects when the daemon answers (5 seconds at most) |
| A daemon answers, and the two have no common protocol version | Runs `~/.nomux/bin/<label>/nomuxd --detach --replace` (20 seconds at most) |

`nomuxd --replace` sends `SIGTERM` to the daemon that has the lock of the runtime directory and waits for the lock. The first daemon answers the requests that it began, removes its socket and ends. The new daemon then finds the holders and takes their leases ([daemon.md](daemon.md)). **No session ends**: a holder is never replaced.
The clients that are attached through the first daemon lose their connection. Their sessions continue.

`daemon status` and `daemon stop` with `--host` start no daemon and replace none, as on the local host.

Limits:

- The client replaces a daemon also if the daemon is newer than the client. Two clients without a common protocol version replace the daemon of each other.
- A session gets the environment of the daemon. The daemon of a remote host gets the environment that `sshd` gives a command: the start files of a login shell are not read.
- `nomuxd` continues after the SSH connection ends, because it has its own session and no file of the connection. A host on which `systemd-logind` has `KillUserProcesses=yes` stops it, and the sessions, when the user logs out.

## Connection policy

- **A command connects one time.** A command that cannot connect fails with the reason. It does not try again: an error of the host key, of the authentication or of the configuration does not go away.
- **An attach connects again.** When the connection of an attach ends without the end of the session, the attach opens a new connection and attaches again with its epoch and the sequence number of the next output byte. The daemon then sends only the bytes that the client does not have ([`spec/frame.md`](https://github.com/nomux-sh/proto/blob/main/spec/frame.md), "Resume"). No output is absent and none comes two times. If the daemon no longer has these bytes, which is more than 256 KiB of output during the outage, the client gets the current screen.
- **Limited attempts with a pause that grows.** The pauses are 0.25, 0.5, 1, 2, 4 and then 8 seconds, for 10 attempts: approximately 48 seconds. After that the attach ends with an error that says how to attach again.
- **One connection attempt at a time.** All the users of one host in a process share one SSH connection, and only one of them connects again at a time.
- **No daemon is started.** An attach that connects again starts no daemon and replaces none. If the daemon stopped, the attach ends with an error, as on the local host.
- **Visible.** The client library reports `ConnectionLost` and `Reconnected`. The CLI saves the title of the terminal window, shows `nomux: connection lost, connecting again` as the title and gives the title back after the connection is there. It does not write to the screen, because the output of the session continues on it.
- **Input is discarded during the outage.** nomux does not type into a session that the user cannot see. The detach keys work.

The local host uses the same code. A daemon that stops ends a local attach with the same error as before.

## What is on the host

| Path | Contents | Mode |
|---|---|---|
| `~/.nomux/bin/<label>/` | `nomux`, `nomuxd`, `nomux-holder` of one version | 0700 |
| `~/.nomux/run/` | The socket `nomuxd.sock`, the lock, the log `nomuxd.log` and one directory for each session | 0700; the socket 0600 |

nomux changes no other file on the host: no shell start file, no `PATH`, no service.
Only the user of the SSH connection can use the directories and the socket. The daemon also closes a connection from a different user.

To remove nomux from a host:

```sh
nomux --host <name> daemon stop     # the sessions continue until they are ended
ssh <ssh-target> 'rm -rf ~/.nomux'  # after the sessions ended
nomux host rm <name>                # on the client: removes the host from config.toml
```

An old version stays in `~/.nomux/bin` until it is removed. Remove a version only when no session uses its holder.

## Configuration file

`nomux host add` saves a host in the global configuration file:

```toml
[hosts.vps]
ssh = "deploy@vps.example.com"
```

| Host | Path |
|---|---|
| Each host, if `XDG_CONFIG_HOME` is an absolute path | `$XDG_CONFIG_HOME/nomux/config.toml` |
| Linux | `~/.config/nomux/config.toml` |
| macOS | `~/Library/Application Support/nomux/config.toml` |

- `hosts.<name>.ssh` is the SSH destination: `[user@]host`, `ssh://[user@]host[:port]`, or a `Host` name of `~/.ssh/config`.
- A name has only the characters `a` to `z`, `A` to `Z`, `0` to `9`, `.`, `_` and `-`, and it starts with a letter or a digit.
- An error names the file and the key, for example ``the key `hosts.vps.shh` is not a known key``.
- nomux keeps each other part of the file, also the comments, when it writes the file. It writes the file with mode 0600 and replaces it in one step.

The code is `nomux_core::configuration`. It reads only the `hosts` table. The other tables of the configuration model are not written.

## Measurements

Measured on Linux x86-64 with the release profile. The SSH server is `sshd` of OpenSSH 10.2 on the same computer, and the client reaches it through a relay program that can add a delay. The program is `tests/measurements/remote.rs`: [tests/README.md](../tests/README.md) says how to run it. Each range is the result of 2 runs.

| Measurement | Result |
|---|---|
| Size of `nomux` (glibc, dynamic) without the SSH client | 943 KiB (966104 bytes) |
| Size of `nomux` (glibc, dynamic) with the SSH client | 3822 KiB (3913456 bytes) |
| Size of `nomuxd` before and after | 881 KiB (901808 bytes), 975 KiB (998176 bytes) |
| Size of `nomux-holder` before and after | 547 KiB (559848 bytes), no change |
| First `nomux host add`: connection, archive, upload of 5.3 MB, checksums, start of the daemon | 0.3 s to 0.7 s |
| Later command `nomux --host vps ls`, median of 20 | 31 ms to 104 ms |
| The same command on the local host | 1 ms to 12 ms |
| Key to echo, SSH to the same computer: median, 99th percentile | 0.3 ms to 3.2 ms, 1.1 ms to 11.6 ms |
| The same during a flood in a second attached session | 0.3 ms to 1.3 ms, 2.6 ms to 10.5 ms |
| Key to echo with a round trip of 100 ms: median, 99th percentile | 100.9 ms, 103.3 ms |
| The same during a flood in a second attached session | 100.7 ms, 107.4 ms |
| Key to echo with a round trip of 100 ms and the loss model: median, 99th percentile | 100.8 ms, 300.9 ms |
| The same during a flood in a second attached session | 101.0 ms, 302.2 ms |
| Output that was lost or came two times in these measurements, and after a cut connection in the tests | None |

- **The dependency cost.** The SSH client adds `russh` and approximately 120 crates that it needs, most of them the RustCrypto algorithm crates. Each license is in the list of `deny.toml`. One of them, `ssh-key`, has only a release candidate version (`0.7.0-rc.12`).
- **Delay.** The relay holds each chunk of bytes for 50 ms in each direction.
- **Loss model.** `tc netem` needs the `root` user and was not used. The relay makes 2% of the chunks wait 200 ms more, and the chunks after them wait behind them. This is what a TCP connection does after a lost packet: it sends the packet again after 200 ms or more, and the later bytes wait. The model does not show the smaller congestion window after a loss. A measurement with real packet loss was not made.
- **With a delay, a key waits one round trip.** nomux shows a key when the host sent its echo. The local echo before the answer of the host is not written.

## Tests

- `crates/nomux-transport/src/ssh`: the reader of `~/.ssh/config`, the patterns, the tokens, the files of known hosts with hashed names, and the SSH destination.
- `crates/nomux-client/src`: the probe, the release target, the checksums, the permissions rule of `host doctor`, and the pauses of the reconnect.
- `tests/remote_host.rs`: the full path with an `sshd` that the test starts. [tests/README.md](../tests/README.md) has the list.

## Open points for the next versions

- **Servers without streamlocal forwarding.** The bridge command as a second way.
- **Configurations that the client does not support.** The fallback to the `ssh` program with `ControlMaster`.
- **Local echo before the answer of the host**, for a connection with a long round trip.
- **Host certificates** and user certificates (`CertificateFile`).
- **Removal of old versions** from `~/.nomux/bin`.
- **The environment of a session** on a remote host.
- **`/etc/ssh/ssh_config`.**
- **QUIC, port forwarding and file transfer.**
