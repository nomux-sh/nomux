//! The sequence scanner of `nomux-term` against the parser of the VT library.
//!
//! `Terminal::incomplete_tail` is the number of bytes at the end of the
//! output that had no effect yet. The daemon starts a client that attaches at
//! that position. The target cuts arbitrary output at each end of a block,
//! and a new parser of the library that starts at the incomplete tail must
//! then make the same calls as the parser that took all the output.
//!
//! The first byte of the input selects the length of the blocks. The rest is
//! the output.

#![no_main]

use alacritty_terminal::vte::{Params, Parser, Perform};
use libfuzzer_sys::fuzz_target;
use nomux_term::{Size, Terminal};

/// Each cut parses the output two times, thus the work for one input is the
/// square of its length.
const MAX_OUTPUT_BYTES: usize = 256;

/// The library puts a character that a cut divided together from 4 bytes.
/// Blocks that are shorter and longer than 4 bytes show each step of that.
const MAX_BLOCK_BYTES: u8 = 8;

fuzz_target!(|input: &[u8]| {
    let Some((block_selector, output)) = input.split_first() else {
        return;
    };
    let output = &output[..output.len().min(MAX_OUTPUT_BYTES)];
    let block_length = 1 + usize::from(block_selector % MAX_BLOCK_BYTES);

    let mut terminal = Terminal::new(Size::new(2, 8).expect("the size is not zero"));
    let mut cut = 0;
    for block in output.chunks(block_length) {
        terminal.feed(block, |_| {});
        cut += block.len();
        let tail = terminal.incomplete_tail();
        assert!(tail <= cut, "tail {tail} is longer than the output {cut}");
        assert_library_agrees(output, block_length, cut, tail);
    }
});

/// One call of the parser of the library.
#[derive(Debug, PartialEq, Eq)]
enum Call {
    Print(char),
    Execute(u8),
    Hook(Vec<Vec<u16>>, Vec<u8>, bool, char),
    Put(u8),
    Unhook,
    Osc(Vec<Vec<u8>>, bool),
    Csi(Vec<Vec<u16>>, Vec<u8>, bool, char),
    Escape(Vec<u8>, bool, u8),
}

#[derive(Default)]
struct Recorder {
    calls: Vec<Call>,
}

fn parameters(params: &Params) -> Vec<Vec<u16>> {
    params.iter().map(<[u16]>::to_vec).collect()
}

impl Perform for Recorder {
    fn print(&mut self, character: char) {
        self.calls.push(Call::Print(character));
    }

    fn execute(&mut self, byte: u8) {
        self.calls.push(Call::Execute(byte));
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        let hook = Call::Hook(parameters(params), intermediates.to_vec(), ignore, action);
        self.calls.push(hook);
    }

    fn put(&mut self, byte: u8) {
        self.calls.push(Call::Put(byte));
    }

    fn unhook(&mut self) {
        self.calls.push(Call::Unhook);
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let params = params.iter().map(|param| param.to_vec()).collect();
        self.calls.push(Call::Osc(params, bell_terminated));
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        let csi = Call::Csi(parameters(params), intermediates.to_vec(), ignore, action);
        self.calls.push(csi);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        let escape = Call::Escape(intermediates.to_vec(), ignore, byte);
        self.calls.push(escape);
    }
}

/// `tail` is the incomplete tail of the terminal at `cut`.
///
/// A new parser that starts at the tail must make, after the cut, the calls
/// of the parser that took the output in the blocks of the terminal. The tail
/// itself must make no call that changes the screen a second time. The data
/// of a DCS sequence and a control character in a sequence are the
/// exceptions: the parser gives them immediately.
fn assert_library_agrees(output: &[u8], block_length: usize, cut: usize, tail: usize) {
    let (before, after) = output.split_at(cut);

    let mut complete = Recorder::default();
    let mut parser = Parser::new();
    for block in before.chunks(block_length) {
        parser.advance(&mut complete, block);
    }
    let calls_before_cut = complete.calls.len();
    parser.advance(&mut complete, after);

    let mut resumed = Recorder::default();
    let mut parser = Parser::new();
    parser.advance(&mut resumed, &before[cut - tail..]);
    let calls_of_tail = resumed.calls.len();
    parser.advance(&mut resumed, after);

    for call in &resumed.calls[..calls_of_tail] {
        let permitted = matches!(call, Call::Execute(_) | Call::Hook(..) | Call::Put(_));
        assert!(permitted, "{call:?} in the tail {tail} at the cut {cut}");
    }
    assert_eq!(
        resumed.calls[calls_of_tail..],
        complete.calls[calls_before_cut..],
        "tail {tail} at the cut {cut}"
    );
}
