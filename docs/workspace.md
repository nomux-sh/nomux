# Workspace tree

This document describes the workspace tree of a host in the core: the Groups and the Projects, the rules that change the tree, the file that keeps it, the Project of a session and the commands of the CLI.
`spec/workspace.md` of the [`proto`](https://github.com/nomux-sh/proto) repository specifies the Workspace service of the control protocol: the requests, the errors, the event, the revision and the limits. This document does not repeat the tables of the specification.

## Model

- A **Project** is a directory on the host, with a name. Two Projects never have the same directory. One Project can have a directory below the directory of a different Project.
- A **Group** has a name and holds Projects and Groups, to any depth.
- Each node (a Group or a Project) is in one Group or at the top level, and has a position among its siblings. The user selects the sequence: the daemon does not sort the nodes.
- Each node has an identifier of 12 characters (lower-case letters and the digits 2 to 7) that the daemon makes when the node comes into the tree. The identifier does not change. 60 random bits make it improbable that a new node gets the identifier of a removed node.
- Two nodes can have the same name, also in the same Group.
- The tree has a revision. Each change increases it by 1, and it never decreases, also after a restart of the daemon.

The tree has no status. A client computes the status of a Group or a Project from the sessions below it: see [Status](#status).

## Code

| Part | Responsibility |
|---|---|
| `nomux_core::workspace_tree` | The rules of the tree: add a node at a position, rename, move with the check for a cycle, remove (a Group gives its content to its parent), the depth-first sequence and the Project of a working directory. The daemon changes its tree with them, and a client applies the events to its copy with the same code. |
| `crates/nomuxd/src/workspace.rs` | The owner of the tree in the daemon: the limits, the identifiers, the resolved directory of a new Project, the refusal of a second Project with the same directory, the revision, the file and the event of each change. |
| `crates/nomuxd/src/workspace/tree_file.rs` | The file of the tree. |
| `crates/nomux-client/src/workspace.rs`, `workspace_requests.rs` | The requests, the event, `Workspace::apply`, which keeps the copy of a client current, and `Session::project`, the Project of a session. |
| `crates/nomuxd/src/sessions/resolved_directories.rs` | The working directory of each session with its symbolic links resolved. |
| `crates/nomux-cli/src/workspace_commands` | The `project` and `group` commands and the names of the nodes on the command line. |

A request that changes the tree changes a copy of it. The daemon writes the copy to the file, and only then takes it as the tree and sends the event. Thus a request that fails, also because the file cannot be written, does not change the tree and makes no event.

## Storage

The daemon keeps the tree in `workspace.json` in its state directory: `~/.local/state/nomux` on Linux and `~/Library/Application Support/nomux` on macOS. The file has the mode `0600`.

```json
{
  "version": 1,
  "revision": 7,
  "nodes": [
    { "id": "qyfketjcadky", "name": "Client work" },
    { "id": "alpwrus76mfj", "parent_id": "qyfketjcadky", "name": "acme-web", "directory": "/home/user/src/acme-web" }
  ]
}
```

| Field | Meaning |
|---|---|
| `version` | The version of the format: 1. |
| `revision` | The revision of the tree. |
| `nodes` | Each node in the sequence of a depth-first walk: a node comes after its parent, and the siblings are in their sequence. |
| `nodes[].parent_id` | The Group of the node. Absent at the top level. |
| `nodes[].directory` | Only a Project has it: the directory with each symbolic link resolved. |

- A write puts the tree in `workspace.json.new`, writes it to the disk (`fsync`), renames it to `workspace.json` and writes the directory to the disk. A stop of the host during a write leaves the old tree or the new tree, never a part of one.
- Without a file, the tree is empty and has the revision 0.
- The daemon does not trust the file. A file that it cannot read, that is not valid JSON, that has a different version (for example from a newer daemon), that breaks a limit or a rule of the tree, does not stop the daemon and is not deleted: the daemon renames it to `workspace.json.<milliseconds>.damaged`, writes a warning with the reason to its log, and starts with an empty tree. The revision of that tree is the time in milliseconds after the Unix epoch, because the revision of the lost tree is not known and must not decrease.
- The daemon reads the file one time, when it starts. It has no timer and no watcher for the file. The tree has 800 nodes at most, thus the memory of the tree is small.

## Rules

`spec/workspace.md` has the full rules. The daemon applies them in this way:

| Request | What the daemon checks | Error |
|---|---|---|
| `AddProject` | The name: 1 to 128 bytes of UTF-8 without a control character. The directory: an absolute path; `canonicalize` resolves each symbolic link; the result is a directory, is UTF-8 and has 1024 bytes at most. No Project has the result as its directory. The tree has fewer than 800 nodes. The parent exists and is a Group. | `FAILED`, or `NOT_FOUND` for an unknown parent |
| `CreateGroup` | The name, the number of nodes and the parent, as for `AddProject`. | `FAILED` or `NOT_FOUND` |
| `RemoveProject`, `RemoveGroup` | The node exists and has the kind of the request. The content of a Group takes the position of the Group in its parent, in its sequence. | `NOT_FOUND` or `FAILED` |
| `Rename` | The node exists, and the name is valid. The same name changes nothing. | `NOT_FOUND` or `FAILED` |
| `Move` | The node and the parent exist, and the parent is a Group. The parent is not the node and not a Group below it: "a Group cannot move into itself or into a Group below it". The same place changes nothing. | `NOT_FOUND` or `FAILED` |

A position (`index`) that is absent or not smaller than the number of siblings puts the node last. For a move, the siblings are the nodes of the new parent without the node itself.

## The Project of a session

A session belongs to the Project whose directory is equal to its resolved working directory or above it. If more than one Project matches, the Project with the longest directory wins. The comparison uses complete path components: `/src/app` matches `/src/app/web`, but not `/src/application`. A session without a working directory, or without a matching Project, has no Project.

- The directory of a Project has its symbolic links resolved, thus the working directory of a session must have them resolved too. Else a session that starts through a link, for example in `/tmp` on macOS (a link to `/private/tmp`), has no Project.
- The daemon resolves the working directory of a session one time: the first time that it describes the session, thus in the response of `Session.Create` and at the first request after the daemon found the session again. It sends the result in `Session.resolved_working_directory`, which then does not change. `Session.working_directory`, the output of `nomux ls` and the working directory of the shell stay the path that the client gave. A path that the daemon cannot resolve (the directory was removed) gives an empty field, and the client then uses the path as it was given.
- `Session::project` in `nomux-client` is the one function with the rule: it takes the resolved path, else the given path, and gives it to `WorkspaceTree::project_of`. `nomux ls` and `nomux status --tree` use it, and so can the desktop client. The Session service has no field for the Project: a client computes it from the session and the tree. The Project of a session changes only when the tree changes, thus a client needs no event for it.

## Status

The status of a Group or a Project is the most urgent aggregate of the sessions below it: the sessions of the Project, or of each Project in the Group and in each Group below it. The priority is the rule of [status.md](status.md#aggregation-and-attention): `needs you` first. `nomux_client::most_urgent` applies it with the code of `nomux-status`. A node without a session has no status.

## Commands

[cli.md](cli.md) has the options, the text output and the JSON documents. A node is named by its identifier or by its path: the names from the top level down to it, with `/` between them, for example `Client work/Acme/acme-web`. The CLI also accepts the name alone if one node has it. A name or a path that more than one node has is refused, with the identifier and the path of each candidate.

```console
$ nomux group new "Client work"
made group 2j3uwgsxeje7: Client work (revision 1)
$ nomux group new Acme --in "Client work"
made group qyfketjcadky: Client work/Acme (revision 2)
$ nomux project add ~/src/acme-web --group "Client work/Acme"
added project alpwrus76mfj: Client work/Acme/acme-web (revision 3)
$ nomux project add ~/src/acme-api --group Acme
added project pb6s6if6sfp3: Client work/Acme/acme-api (revision 4)
$ nomux project add ~/src/notes
added project t65uigu3nixn: notes (revision 5)
$ nomux group move notes --to Acme --before acme-web
moved t65uigu3nixn: Client work/Acme/notes (revision 6)
$ nomux group move Acme --to Acme
nomux: a Group cannot move into itself or into a Group below it
$ nomux group ls --tree
NAME          KIND     ID            DIRECTORY
Client work   group    2j3uwgsxeje7  -
  Acme        group    qyfketjcadky  -
    notes     project  t65uigu3nixn  /home/user/src/notes
    acme-web  project  alpwrus76mfj  /home/user/src/acme-web
    acme-api  project  pb6s6if6sfp3  /home/user/src/acme-api
$ nomux group rm "Client work"
removed group 2j3uwgsxeje7: Client work (revision 7)
```

- `project add` without a directory adds the current directory. On the local host, a directory in a git repository adds the root of the repository (`git rev-parse --show-toplevel`). With `--host`, the directory is necessary and is an absolute path on the remote host.
- `group rename` and `group move` take a Group or a Project.
- `group move` needs `--to <group>`, `--top`, `--before <node>` or `--after <node>`. Without `--to` and `--top`, the node goes to the Group of the sibling of `--before` or `--after`.
- There is no `nomux tree` alias. `group ls --tree` lists the tree, and `status --tree` shows its status: one name for each function.

## Events and two clients

Each change makes one `WorkspaceChanged` event with the new revision. A client keeps its copy of the tree with the procedure of the specification: `Workspace::apply` ignores an event with a revision that the copy has, applies the next revision, and returns `Applied::Missed` for a gap, for a change that does not fit the copy, and for a change that the client does not know. After `Missed` and after a snapshot, the client asks for the tree again. `events --follow --filter workspace-changed` prints the events.

## Measurements

The targets of [bench/README.md](../bench/README.md) hold with a full tree: the start of a session does not read the tree, and the daemon reads the file only when it starts. Measured on Linux (Fedora 44, 8 cores) with a release build. The tree of 800 nodes has the largest names (128 bytes) and, for each Project, a directory of 1001 bytes: the largest tree of the limits.

| Tree | Resident memory of the idle daemon | CPU time of the daemon in 30 s, idle | `nomux new`, warm pool, median of 30 | `group ls --tree`, median of 10 |
|---|---|---|---|---|
| 0 nodes | 4.6 MB | 0 clock ticks | 2.4 to 2.7 ms | 1.5 ms |
| 800 nodes | 7.4 to 8.2 MB | 0 clock ticks | 2.0 to 4.0 ms | 7.1 to 7.8 ms |

The target for an idle daemon is less than 10 MB, and for a new session with the warm pool less than 10 ms.

## Open points for the next versions

- The open items of the specification: the default agent and the color of a Project, a Worktree outside the directory of its Project, a field of a session with its Project, and a request that changes more than one node.
- The group row in the sidebar of the desktop client, which opens and closes a Group and shows its status.
- A check for the directory of a Project that went away: the Project stays in the tree.
