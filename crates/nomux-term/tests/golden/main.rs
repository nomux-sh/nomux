//! Golden tests: text snapshots of the visible screen after the sequences
//! that full-screen programs and agent TUIs send.
//!
//! Each test gives one sequence to a new terminal and compares the screen
//! with the snapshot in the test. `snapshot.rs` has the format. A test fails
//! if the VT library or the conversion of `nomux-term` reads a sequence in a
//! different way.
//!
//! The expected screens come from the DEC VT documentation and from the
//! xterm and kitty specifications of the sequences. `tests/redraw.rs` has the
//! contract of the redraw sequence: these tests do not repeat it.
//!
//! `conformance.rs` runs the conformance cases of the directory `cases`,
//! which are data: the sequence and the screen of xterm.

mod alternate_screen;
mod conformance;
mod cursor_motion;
mod erase;
mod insert_and_delete;
mod line_drawing;
mod modes;
mod scroll_region;
mod sgr;
mod snapshot;
mod tab_stops;
mod wide_and_combining;
