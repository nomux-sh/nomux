# docs

This directory contains the developer notes of the core: architecture, design notes and decisions that a contributor to this repository must know.

The user documentation is not here. It is in the [`docs`](https://github.com/nomux-sh/docs) repository.

## Contents

| File | Subject | State |
|---|---|---|
| [architecture.md](architecture.md) | Components of the core and the responsibilities of the holder, the daemon and the client | in use |
| [holder.md](holder.md) | The holder: process model, ring buffer, control protocol, discovery files | in use |
| [daemon.md](daemon.md) | The daemon: process model, socket, lifecycle, how it finds and holds the holders | in use |
| [cli.md](cli.md) | The CLI: commands, JSON schema, exit codes | in use |
| [extensions.md](extensions.md) | The extension host: installed extensions, life cycle of a process, isolation, limits, and the host in the daemon with its directory, the `Ext` service and the host services | in use |
| [remote.md](remote.md) | Remote hosts through SSH: the SSH client, the supported `~/.ssh/config`, the trust rules, the channels, the installation on the host, the reconnect | in use |

## Status

The directory has the architecture overview and the notes for the holder, the daemon, the CLI, the remote hosts and the extensions.
The notes for a component come together with its code. The notes for the journal and the attach sequence are the next ones. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).

## License

The documents in this directory are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
