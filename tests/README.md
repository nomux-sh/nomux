# tests

This directory is for the end-to-end tests of the core. An end-to-end test starts a real `nomux-holder` and a real `nomuxd`, and it uses the `nomux` CLI as the client.

## Status

Structure only. There is no end-to-end test, because the binaries only print their version.
The first tests come with the holder and the daemon. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md). Examples of the planned tests:

- Start a session, detach, attach again, and compare the screen.
- Stop `nomuxd` with `kill -9`, start it again, and make sure that the session is alive.

## Where the current tests are

- Unit tests are in the source file of the code that they test.
- The test of the version output is in `crates/nomux-cli/tests/version.rs`. It runs the real `nomux` binary.

Run all tests with `cargo nextest run --workspace`.
