# fuzz

This directory has the [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) targets of the core.
The targets give arbitrary bytes to the code that reads bytes from a process that nomux does not trust: the output of a program, and the socket and the shared memory of a holder.

## Targets

Each target is one file in `fuzz_targets`. The comment at the top of the file has the format of its input.

| Target | Code | A run fails if |
|---|---|---|
| `terminal_feed` | `Terminal::feed` and `Terminal::resize` of `nomux-term`, and the parsers of `nomux_term::osc` (OSC 7501, 133, 9, 777) | The terminal or a parser panics, the cursor is not on the screen, a cell of the screen cannot be read, or the process uses more memory than the limit. |
| `redraw_round_trip` | `Terminal::write_redraw` of `nomux-term` | A new terminal that gets the redraw sequence does not have the same cells, cursor and modes. This is the contract of attach. |
| `incomplete_tail` | `Terminal::incomplete_tail` of `nomux-term` | A parser of the VT library that starts at the incomplete tail does not make the same calls as a parser that took all the output. The target examines each end of a block of the output. |
| `holder_wire_decode` | `FrameBuffer`, `Hello::decode`, `Request::decode` and `Reply::decode` of `nomux-holder-protocol` | A decoder panics or allocates more than the limit, or a message is different after `encode` and a second `decode`. |
| `program_reports_decode` | `ProgramReports::decode` of `nomux-holder-protocol` | The decoder panics or allocates more than the limit, or the program reports are different after `encode` and a second `decode`. |
| `ring_reader` | `RingReader` of `nomux-holder-protocol` | The reader panics on a region with an arbitrary header, length and positions, or a read returns a byte that is not the byte of the region at that position. |
| `journal_reader` | `JournalReader` of `nomux-holder-protocol`, which the daemon and the holder use to read the journal | The reader panics or allocates more than the limit on segment files that were cut, changed, copied or forged, a range or a checkpoint is not a possible answer, the records before the first changed byte of a segment are not readable, or, without a forged byte, the reader returns a byte that the writer did not write at that position. |
| `osc_records` | The parsers of `nomux_term::osc` (OSC 7501, 133, 9, 777), without the terminal | A parser panics, or a record has a field that its documentation does not permit: a text that is not one line or that is longer than its limit, a name with a byte that a name cannot have, or a `kind` or a `progress` with a state that does not have it. |

`src/terminal_script.rs` has the input format that `terminal_feed` and `redraw_round_trip` share: the output of a program, with resizes between its parts.

`src/journal_script.rs` has the input format of `journal_reader`: a script. The checksums of the journal stop arbitrary bytes at the first header, thus the target writes valid records with the encoder of the holder, and the script then cuts the files, changes bits, copies a file to the name of a different segment, and appends records and files of a hostile writer whose checksums are correct. `src/written_journal.rs` keeps what a reader must find after each step.

`terminal_feed` also reaches the OSC parsers, but only with a payload that the terminal lets through, and it examines no record. In a measurement, `terminal_feed` did not find in 60 seconds a planted defect in the parser of OSC 7501 that `osc_records` found in less than 1 second.

## Run a target

Install the tools one time. `rust-toolchain.toml` in this directory pins the nightly compiler that `cargo-fuzz` needs, and rustup installs it at the first run.

```sh
cargo install --locked cargo-fuzz@0.13.2
```

Run one target for a number of seconds, from the root of the repository:

```sh
cargo xtask fuzz terminal_feed 60
```

The command starts with the committed inputs in `seeds/<target>` and keeps the inputs that it finds in `corpus/<target>`.
A run stops at the first failure and writes the input to `artifacts/<target>`. Git ignores `corpus` and `artifacts`.

The command sets two limits of libFuzzer: 1024 MB of memory for the process and 64 MB for one allocation.
A target that goes over a limit fails, because the code has no bound for its input.

To run a failure again and to make its input smaller, use `cargo-fuzz` in this directory:

```sh
cargo fuzz run terminal_feed artifacts/terminal_feed/<file>
cargo fuzz tmin terminal_feed artifacts/terminal_feed/<file>
```

## A failure

1. Make the input smaller with `cargo fuzz tmin`.
2. Open an issue with the label `bug` that has the input and a reproducer in Rust.
3. Correct the code, and add a test with the reproducer to the crate.
4. If the target cannot pass before the correction, make the target skip that one case and put the link to the issue in a comment. Remove the skip together with the correction.

## Cases that the targets skip

No target skips a case now.

`terminal_feed` and `redraw_round_trip` also select a resize to 1 column. `nomux-term` refuses that size, thus the terminal keeps its size, as the terminal of a holder that refuses a `Resize`.

`ring_reader` does not make the region shorter after the reader mapped it.

`journal_reader` writes records with a payload of 512 bytes at most, because one record of the largest length makes a run 30 times slower. It does not remove a file while a reader reads.

## Seeds

`seeds/<target>` has a small number of valid inputs for each target: terminal output with each kind of sequence, one frame of each message kind, program reports, regions with a valid header, journal scripts with each kind of step, and payloads of each OSC code.
The files are small, because a run needs only one example of each structure. Do not commit a corpus that a run made.

## Dependencies

This workspace is for development only. It is not in the release graph: no dependency of this workspace is linked into a release binary, and `cargo deny check` of the core workspace does not read it.

`libfuzzer-sys` has the license `(MIT OR Apache-2.0) AND NCSA`. NCSA is the permissive license of libFuzzer of the LLVM project, and it is not in the list of `deny.toml`.
Each other dependency has a license from the list.

## CI

The workflow is `.github/workflows/fuzz.yml`.

- For each pull request that changes `fuzz`, `xtask`, `crates/nomux-term` or `crates/nomux-holder-protocol`: 10 seconds for each target, 80 seconds in total.
- Each night: 10 minutes for each target.

A failure fails the job, and the job uploads `artifacts` as the `fuzz-artifacts` artifact of the run.

## Planned targets

These targets come together with the code that they test. Refer to the [roadmap](https://github.com/nomux-sh/docs/blob/main/src/content/docs/project/roadmap.md).

- The terminal frame codec of the `nomux-proto` crate, when the daemon and the clients use it for terminal data.
- The configuration parser.
