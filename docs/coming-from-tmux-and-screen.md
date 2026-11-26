# Coming from tmux and screen

This page maps the commands of tmux and GNU screen that people type most to the commands of `nomux`. [cli.md](cli.md) has each command with its options, its output and its exit codes.

A session in nomux has an identifier and can have a name. Each command that takes a session takes a target: the identifier, the name, or the start of one of the two ([cli.md](cli.md#targets-and-names)). Thus `nomux attach api` and `nomux attach k3xq` both work.

## From tmux

| You type in tmux | In nomux |
|---|---|
| `tmux new -s api` | `nomux new -n api -a` |
| `tmux new -d -s api 'cmd'` | `nomux new -n api -- cmd` (`new` does not attach without `-a` or `-A`) |
| `tmux new -A -s api` | `nomux new -A -n api` |
| `tmux attach -t api` | `nomux attach api` or `nomux a api` (`-t api` works too) |
| `tmux attach` | `nomux attach`, or `nomux` on a terminal: the session that a client attached to last |
| `tmux attach -d -t api` | `nomux attach -d api` |
| `tmux attach -r -t api` | `nomux attach -r api` |
| `tmux ls` | `nomux ls` |
| `tmux ls -F '#{session_name}'` | `nomux ls --json \| jq -r '.sessions[].name'` |
| `tmux rename -t api web` | `nomux rename api web` |
| `tmux kill-session -t api` | `nomux kill api` |
| `tmux kill-server` | `nomux kill --all` (the sessions; it asks on a terminal) and `nomux daemon stop` (the daemon: the sessions continue) |
| `tmux detach -s api` | `nomux detach api` |
| `tmux send-keys -t api 'ls' Enter` | `nomux send api 'ls'` |
| `tmux send-keys -l -t api 'x'` | `nomux send --no-enter api 'x'` |
| `tmux send-keys -t api C-c` | `nomux send -k api C-c` |
| `tmux pipe-pane -O 'cat >> f'` | `nomux capture -f api >> f` |
| `tmux capture-pane -p -S - -E - -t api` | `nomux capture --lines 100000 api` (the output as the command wrote it, with its escape sequences) |
| `ssh host tmux attach -t api` | `nomux --host host attach api` |
| `C-b d` | `Ctrl-a d` (`Ctrl-\ d` works too) |
| `C-b C-b` (send `C-b`) | `Ctrl-a Ctrl-a` sends `Ctrl-a` |
| `set -g prefix C-b` | `nomux attach --prefix C-b`, `NOMUX_PREFIX=C-b`, or `prefix = ["C-b"]` in `[attach]` of `config.toml` |

Different on purpose:

- nomux has no windows, panes or key bindings. The desktop client has the layout.
- `-d` in tmux `new` means "start detached". `nomux new` never attaches without `-a` or `-A`, and `-d` of `nomux attach` means "detach the others", as in `tmux attach -d`.
- `nomux kill` has no `-a`: `tmux kill-session -a` ends each other session. `nomux kill --all` ends each session, also the current one.
- A name is unique on the host, as in tmux. An exact name wins over a longer name with the same start, and a start that more than one session has is refused with the candidates.
- The sessions continue after a restart of the host as dormant sessions: `nomux ls` shows them, and `attach` or `start` starts them again.

## From screen

| You type in screen | In nomux |
|---|---|
| `screen -S api` | `nomux new -n api -a` |
| `screen -dmS api cmd` | `nomux new -n api -- cmd` |
| `screen -ls` | `nomux ls` |
| `screen -r api` | `nomux attach api` (`-r` of `nomux attach` means read-only) |
| `screen -r` | `nomux attach`, or `nomux` on a terminal |
| `screen -d -r api` | `nomux attach -d api` |
| `screen -x api` | `nomux attach api` (more than one terminal can attach to one session) |
| `screen -dRR api` | `nomux new -A -n api` |
| `screen -d api` | `nomux detach api` |
| `screen -S api -X sessionname web` | `nomux rename api web` |
| `screen -S api -X quit` | `nomux kill api` |
| `screen -S api -X stuff 'ls\r'` | `nomux send api 'ls'` |
| `screen -S api -X stuff $'\003'` | `nomux send -k api C-c` |
| `screen -L -Logfile f` | `nomux capture -f api >> f` (the daemon keeps the output anyway) |
| `screen -S api -X hardcopy -h f` | `nomux capture --lines 100000 api > f` |
| `screen -wipe` | Not necessary: the daemon removes an ended session after 24 hours (`sessions.ended_retention`), and `nomux kill` removes a dormant session |
| `C-a d` | `Ctrl-a d` (`Ctrl-\ d` works too) |
| `C-a a` (send `C-a`) | `Ctrl-a a` or `Ctrl-a Ctrl-a` |

Different on purpose:

- A name is unique on the host. screen permits two sessions with one name.
- An exact name always wins over a longer name with the same start. In screen, `-S build` is ambiguous when `build` and `builder` exist.
- There is no configuration file such as `.screenrc`.
