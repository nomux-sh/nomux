# fuzz

This directory is for the `cargo-fuzz` targets of the core.

## Status

Milestone M0 contains no fuzz target. There is no parser to fuzz yet.
Milestone M1 adds the targets together with the code that they test:

- the VT parser
- the OSC parser (OSC 7501, 133, 9, 777)
- the terminal frame codec
- the configuration parser

## Plan for CI

- For each pull request: a smoke run of 60 seconds for each target.
- Each night: a long run.

`cargo-fuzz` needs a nightly compiler. Thus the fuzz crate will not be a member of the workspace, which uses the pinned stable toolchain.
