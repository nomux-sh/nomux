//! Compares the scanner with the parser of the VT library. Each output is
//! cut at each byte offset, and a second time a few bytes later.

use alacritty_terminal::vte::{Params, Parser, Perform};

use super::SequenceScanner;
use crate::osc::Osc;

/// One call of the parser of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
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
    actions: Vec<Action>,
}

fn parameters(params: &Params) -> Vec<Vec<u16>> {
    params.iter().map(<[u16]>::to_vec).collect()
}

impl Perform for Recorder {
    fn print(&mut self, character: char) {
        self.actions.push(Action::Print(character));
    }

    fn execute(&mut self, byte: u8) {
        self.actions.push(Action::Execute(byte));
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        let hook = Action::Hook(parameters(params), intermediates.to_vec(), ignore, action);
        self.actions.push(hook);
    }

    fn put(&mut self, byte: u8) {
        self.actions.push(Action::Put(byte));
    }

    fn unhook(&mut self) {
        self.actions.push(Action::Unhook);
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let params = params.iter().map(|param| param.to_vec()).collect();
        self.actions.push(Action::Osc(params, bell_terminated));
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        let csi = Action::Csi(parameters(params), intermediates.to_vec(), ignore, action);
        self.actions.push(csi);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        let escape = Action::Escape(intermediates.to_vec(), ignore, byte);
        self.actions.push(escape);
    }
}

/// The code and the payload of each OSC sequence that the scanner gives for
/// `parts`.
fn scanned_osc(scanner: &mut SequenceScanner, part: &[u8], found: &mut Vec<(u32, Vec<u8>)>) {
    scanner.scan(part, |osc| found.push((osc.code, osc.payload.to_vec())));
}

/// The OSC sequences of the library that have a decimal code.
fn library_osc(actions: &[Action]) -> Vec<(u32, Vec<u8>)> {
    let sequences = actions.iter().filter_map(|action| match action {
        Action::Osc(params, _) => Some(params.join(&b';')),
        _ => None,
    });
    sequences
        .filter_map(|sequence| Osc::parse(&sequence).map(|osc| (osc.code, osc.payload.to_vec())))
        .collect()
}

/// The scanner and the parser of the library get `output` in three parts:
/// to `first_cut`, to `cut`, and the rest. The scanner gives the incomplete
/// tail at `cut`.
///
/// A new parser that starts at the incomplete tail must do, after the cut,
/// the same calls as the parser that took all the output. The tail itself
/// must do no call that changes the screen a second time. The data of a DCS
/// sequence and a control character in a sequence are the exceptions: the
/// parser gives them immediately.
fn assert_tail_at(output: &[u8], first_cut: usize, cut: usize, compare_osc: bool) {
    let (before, after) = output.split_at(cut);
    let (first, second) = before.split_at(first_cut);
    let mut scanner = SequenceScanner::new();
    let mut found = Vec::new();
    scanned_osc(&mut scanner, first, &mut found);
    scanned_osc(&mut scanner, second, &mut found);
    let tail = scanner.incomplete_tail();
    scanned_osc(&mut scanner, after, &mut found);

    let mut library = Recorder::default();
    let mut parser = Parser::new();
    parser.advance(&mut library, first);
    parser.advance(&mut library, second);
    let calls_before_cut = library.actions.len();
    parser.advance(&mut library, after);

    let mut resumed = Recorder::default();
    let mut resumed_parser = Parser::new();
    resumed_parser.advance(&mut resumed, &before[cut - tail..]);
    let calls_of_tail = resumed.actions.len();
    resumed_parser.advance(&mut resumed, after);

    let context = format!("cuts {first_cut} and {cut}, tail {tail}, output {output:x?}");
    for action in &resumed.actions[..calls_of_tail] {
        let permitted = matches!(
            action,
            Action::Execute(_) | Action::Hook(..) | Action::Put(_)
        );
        assert!(permitted, "{action:?} in the tail: {context}");
    }
    assert_eq!(
        resumed.actions[calls_of_tail..],
        library.actions[calls_before_cut..],
        "{context}"
    );
    if compare_osc {
        assert_eq!(found, library_osc(&library.actions), "{context}");
    }
}

/// The library puts a character that a cut divided together from 4 bytes.
/// A second cut in these bytes shows each step of that.
const SECOND_CUT_DISTANCE: usize = 6;

/// Checks each cut of `output`, and each second cut near it.
fn assert_tail_at_each_cut(output: &[u8], compare_osc: bool) {
    for first_cut in 0..=output.len() {
        let last_cut = output.len().min(first_cut + SECOND_CUT_DISTANCE);
        for cut in first_cut..=last_cut {
            assert_tail_at(output, first_cut, cut, compare_osc);
        }
    }
}

/// Each kind of sequence that the scanner knows, and text between them.
const SEQUENCES: &[&[u8]] = &[
    b"plain text\r\n\ttab",
    // CSI: parameters, subparameters, a private marker, an intermediate.
    b"\x1b[1;38;2;10;20;30mcolour\x1b[0m\x1b[4:3m\x1b[?1049h\x1b[2;3H\x1b[1 q\x1b[>1u\x1b[2@",
    // UTF-8: 2, 3 and 4 bytes, and a combining mark.
    "caf\u{e9} \u{20ac} \u{1f600} e\u{301} \u{4e2d}".as_bytes(),
    // OSC with the two terminators.
    b"\x1b]0;title\x07\x1b]133;A\x1b\\$ \x1b]7501;state=working;kind=question\x07",
    b"\x1b]8;;https://nomux.sh\x1b\\link\x1b]8;;\x1b\\\x1b]\x07\x1b]no code\x07",
    // DCS with parameters, with an intermediate, ignored, and with the
    // one-byte string terminator.
    b"\x1bPq#0;2;0;0;0#0~~\x1b\\\x1bP1;2$rdata\x1b\\\x1bP1<ignored~\x9cignored\x1b\\\x1bP+q544e\x9ctext",
    b"\x1bP$0q\x9cignored\x1b\\\x1bP/q\x9c\x1bP@\x9c\x1bP1 q\x9c\x1bP?1q\x9ctext\x1bP?<q\x9cignored\x1b\\",
    // APC, PM and SOS strings.
    b"\x1b_Gf=24;payload\x1b\\\x1b^private\x07 message\x1b\\\x1bXstring\x9c[m\x1b\\",
    // ESC with a final byte, and with intermediates.
    b"\x1b7\x1bM\x1b=\x1b(B\x1b#8\x1b F\x1b(\x7f0",
    // A sequence that starts in a sequence.
    b"\x1b[31\x1b[32m\x1b]2;title\x1b[1m\x1bPq\x1b]133;B\x07\x1b\x1b[m",
    // CAN and SUB end a sequence. A control character in a sequence.
    b"\x1b[3\x18m\x1b]2;cut\x1atext\x1bPq\x18text\x1b[1\r\nm\x1b\x08c",
    // Bytes that are not valid UTF-8, also before an escape character.
    b"\xff\xc3(\xe2\x82\x1b[m\x80\xf0\x9f\xe2\x82\xac\xed\xa0\x80\xc3",
    // After a cut in a character, the library can skip the bytes that come
    // after that character: here the escape character.
    b"text\xc3\xa9\x1b\xff[mtext\xe2\x82\xac\x1b\xff]0;no\x07\xf0\x9f\x98\x1b[m",
];

#[test]
fn the_incomplete_tail_agrees_with_the_library_at_each_cut_of_the_known_sequences() {
    assert_tail_at_each_cut(&SEQUENCES.concat(), true);
}

/// The bytes that change the state of the parser, and some text.
const STATE_BYTES: &[u8] = b"\x1b\x1b\x1b\x1b[[]]PPX^_\\\x07\x18\x1a\x9c\x9c\x7f\x00\r\n /0123;:?<@$#(m~hHqa\xc3\xa9\xe2\x82\xac\xf0\x9f\x98\x80\x80\xff";

/// A fixed sequence of numbers that looks random (xorshift).
struct Numbers(u64);

impl Numbers {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % 1_000_003).expect("the number is small")
    }
}

#[test]
fn the_incomplete_tail_agrees_with_the_library_at_each_cut_of_generated_bytes() {
    let mut numbers = Numbers(0x9E37_79B9_7F4A_7C15);
    for _ in 0..48 {
        let output: Vec<u8> = (0..256)
            .map(|_| STATE_BYTES[numbers.next() % STATE_BYTES.len()])
            .collect();
        // The library keeps 16 parameters of an OSC sequence at most, thus
        // the payload of a generated sequence can be different.
        assert_tail_at_each_cut(&output, false);
    }
}

#[test]
fn the_tail_is_the_sequence_or_the_character_that_is_not_complete() {
    let tail_of = |output: &[u8]| {
        let mut scanner = SequenceScanner::new();
        scanner.scan(output, |_| {});
        scanner.incomplete_tail()
    };

    assert_eq!(tail_of(b"text"), 0);
    assert_eq!(tail_of(b"text\x1b"), 1);
    assert_eq!(tail_of(b"text\x1b[38;5"), 6);
    assert_eq!(tail_of(b"text\x1b[38;5;1m"), 0);
    assert_eq!(tail_of(b"\x1b]0;title"), 9);
    assert_eq!(tail_of(b"\x1b]0;title\x1b"), 1);
    assert_eq!(tail_of(b"\x1bPqdata"), 7);
    assert_eq!(tail_of(b"\x1b_payload"), 9);
    assert_eq!(tail_of(b"\x1b("), 2);
    assert_eq!(tail_of(b"a\xe2\x82"), 2);
    assert_eq!(tail_of(b"a\xe2\x82\xac"), 0);
    assert_eq!(tail_of(b"\xf0\x9f\x98"), 3);
}
