# bench

This directory is for the performance tests and the idle CPU tests of the core.

## Status

Structure only. There is no benchmark, because there is nothing to measure before the holder and the daemon exist.
The first benchmarks come with the holder and the daemon, together with the CI job that enforces the targets below. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).

## Targets

When the benchmarks exist, CI measures each target that applies to this repository.
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

The planned tools are `criterion`, `vtebench`, `hyperfine`, typometer, flood tests, `tc netem`, and `heaptrack` or `dhat`.

Shared CI runners change the measurements.
Thus the benchmark job runs on a dedicated runner: a short set for each pull request and the full set each night.
