# tests

This directory is for the end-to-end tests of the core. An end-to-end test starts a real `nomux-holder` and a real `nomuxd`, and it uses the `nomux` CLI as the client.

## Status

Milestone M0 contains no end-to-end test. The binaries only print their version.
Milestone M1 adds the first tests, for example:

- Start a session, detach, attach again, and compare the screen.
- Stop `nomuxd` with `kill -9`, start it again, and make sure that the session is alive.

## Where the current tests are

- Unit tests are in the source file of the code that they test.
- The tests of the version output are in `crates/<crate>/tests/version.rs` of each binary crate.

Run all tests with `cargo nextest run --workspace`.
