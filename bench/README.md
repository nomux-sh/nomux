# bench

This directory is for the performance tests and the idle CPU tests of the core.

## Status

One benchmark exists: [`vt-candidates`](vt-candidates), which compares the VT library candidates and measures `nomux-term`.
[ADR-0003](https://github.com/nomux-sh/docs/blob/main/adr/0003-vt-library.md) has the results and the decision.

The benchmarks of the holder and the daemon come with these components, together with the CI job that enforces the targets below. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).

## Layout

Each benchmark is a directory with its own Cargo workspace.
The core workspace does not have these workspaces as members, thus `cargo xtask ci` and the pull request CI do not build them, and their dependencies are not in the `Cargo.lock` of the core.

| Path | Contents |
|---|---|
| `vt-candidates/probe` | Library `vt-probe`: the measurements that each probe runs, the corpus generators and the conformance cases. |
| `vt-candidates/candidates/<name>` | One probe binary for each candidate. A probe connects one VT library to `vt-probe` and has no other code. |
| `vt-candidates/runner` | Binary `vt-bench`: builds the probes, runs them and prints the report. |

## VT library benchmark

Run it from the root of the repository:

```sh
cargo run --release --manifest-path bench/vt-candidates/Cargo.toml
```

The command prints the report as Markdown tables on the standard output. A full run takes approximately 2 minutes after the first build.

Requirements:

- Linux: the musl target of the host architecture, for example `rustup target add x86_64-unknown-linux-musl`. On macOS the probes use the host target.
- The `ps` program.
- Optional: Zig 0.15.2, git and network access. The build of `libghostty-vt` needs them. If `zig` is not installed, the report says that this candidate was not measured.

The probes:

| Probe | Candidate |
|---|---|
| `probe-baseline` | No VT library. The reference for size and memory. |
| `probe-vte` | `vte`, a parser without screen state. |
| `probe-vt100` | `vt100`. |
| `probe-vt100-psmux` | `vt100-psmux`, the fork of `vt100` that the psmux project keeps. |
| `probe-alacritty-terminal` | `alacritty_terminal`. |
| `probe-nomux-term` | `nomux-term` of this repository: the selected library behind the interface of nomux. It keeps no scrollback. |
| `probe-libghostty-vt` | `libghostty-vt`. |

The manifest of each probe pins the version of its candidate.

What the report shows:

| Table | Measurement |
|---|---|
| Parse throughput | MiB/s for each corpus. A probe feeds 16 MiB to a new terminal of 80 x 24 cells with 1000 lines of scrollback, in blocks of 64 KiB. The value is the median of 5 runs. |
| Memory of one session | Resident set size of the probe process, from `ps`, while it keeps one terminal of 80 x 24 cells: with a full screen only, and with 1000 and 10000 lines of scrollback that are full. The value in brackets is the difference from the baseline probe. |
| Size and build requirements | Size of the stripped probe binary with the release profile of the core and with the same profile at `opt-level = "z"`. The value in brackets is the difference from the baseline probe. "crates" is the number of crates that the candidate adds to the binary. |
| Conformance | Sequences that full-screen programs and agent TUIs send. A case passes if the screen text and the cursor position are correct. |

The corpora are generated from a fixed seed, thus the repository has no corpus files:

| Corpus | Contents |
|---|---|
| `plain-text` | Lines of words, as a build log. |
| `sgr-color` | Lines in which each word has its own colors and attributes. |
| `cursor-motion` | Cursor addressing, erase and line operations, as a full-screen program. |
| `unicode` | Wide characters, combining marks, emoji and box drawing. |
| `scroll-flood` | Very short lines: the screen scrolls for almost each byte. |

On Linux the probes are static musl binaries, as the release binaries of the holder.
The benchmark has no GPL test suite. `esctest` is not used.

## Targets

When the benchmarks of the holder and the daemon exist, CI measures each target that applies to this repository.
If a measurement is worse than its target, the build fails.

| Measurement | Target | Owner |
|---|---|---|
| Local key press to screen | < 10 ms | core + desktop |
| Remote key press, visible (predictive echo) | < 10 ms; real confirmation in RTT + 5 ms | core + desktop |
| Output flood of 10 MB/s | UI at 60 fps or more; input latency does not change | core + desktop |
| New session start (warm pool) | < 10 ms | core |
| Network change | The connection stays open | core |
| Reconnect after a disconnect | < 1 s, no data loss | core |
| Daemon update or daemon crash | No session stops | core |
| First screen after a reboot | < 1 s (lazy restore) | core |
| RAM of an idle `nomuxd` | < 10 MB | core |
| RAM of one holder for each session | < 3 MB (scrollback is on disk) | core |
| Size of the `nomux-holder` binary | < 1 MB | core |
| Size of the `nomuxd` binary | < 10 MB | core |
| Desktop cold start | < 300 ms | desktop |
| Desktop idle RAM | < 80 MB; in the background < 40 MB | desktop |
| Desktop idle CPU | Approximately 0 % (budget in CI) | desktop |

The `desktop` repository measures the targets that have the owner "desktop".
The targets that have the owner "core + desktop" have a core part (holder, daemon, transport) that this directory measures.

## Tools

The planned tools for the benchmarks of the holder and the daemon are `criterion`, `vtebench`, `hyperfine`, typometer, flood tests, `tc netem`, and `heaptrack` or `dhat`.

Shared CI runners change the measurements.
Thus the benchmark job runs on a dedicated runner: a short set for each pull request and the full set each night.
