# fuzz

This directory is for the `cargo-fuzz` targets of the core.

## Status

Structure only. There is no fuzz target, because there is no parser to fuzz yet.
Each target comes together with the code that it tests. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md). The planned targets are:

- the VT parser
- the OSC parser (OSC 7501, 133, 9, 777)
- the terminal frame codec
- the configuration parser

## Plan for CI

- For each pull request: a smoke run of 60 seconds for each target.
- Each night: a long run.

`cargo-fuzz` needs a nightly compiler. Thus the fuzz crate will not be a member of the workspace, which uses the pinned stable toolchain.
