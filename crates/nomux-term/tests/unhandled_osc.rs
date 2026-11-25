//! The contract of the `unhandled_osc` argument of `Terminal::feed`: the
//! caller gets the OSC sequences that the VT library does not handle.

use nomux_term::{Size, Terminal};

/// Feeds each block to a new terminal and returns the code and the payload
/// of each OSC sequence that the caller gets.
fn unhandled_osc(blocks: &[&[u8]]) -> Vec<(u32, Vec<u8>)> {
    let mut terminal = Terminal::new(Size::new(5, 20).expect("the size is not zero"));
    let mut found = Vec::new();
    for block in blocks {
        terminal.feed(block, |osc| found.push((osc.code, osc.payload.to_vec())));
    }
    found
}

#[test]
fn caller_gets_an_unknown_code_with_its_payload() {
    let found = unhandled_osc(&[b"text\x1b]7501;state=working;kind=question\x07text"]);

    assert_eq!(found, [(7501, b"state=working;kind=question".to_vec())]);
}

#[test]
fn caller_gets_each_sequence_with_both_terminators() {
    let found = unhandled_osc(&[
        b"\x1b]133;A\x1b\\$ \x1b]133;B\x07\x1b]9;done\x1b\\\x1b]777;notify;title;body\x07",
    ]);

    assert_eq!(
        found,
        [
            (133, b"A".to_vec()),
            (133, b"B".to_vec()),
            (9, b"done".to_vec()),
            (777, b"notify;title;body".to_vec()),
        ]
    );
}

#[test]
fn caller_gets_a_sequence_that_is_split_between_calls() {
    let sequence = b"\x1b]7501;state=done\x1b\\";
    let bytes: Vec<&[u8]> = sequence.chunks(1).collect();

    assert_eq!(unhandled_osc(&bytes), [(7501, b"state=done".to_vec())]);
}

#[test]
fn caller_does_not_get_the_codes_of_the_library() {
    let found = unhandled_osc(&[
        b"\x1b]2;title\x07\x1b]8;;https://nomux.sh\x1b\\link\x1b]8;;\x1b\\\x1b]52;c;dGV4dA==\x07",
    ]);

    assert_eq!(found, []);
}

#[test]
fn bracket_outside_of_an_escape_sequence_starts_no_sequence() {
    // The `]` comes after a complete CSI sequence, thus it is text.
    let found = unhandled_osc(&[b"\x1b[1m]133;A\x07"]);

    assert_eq!(found, []);
}

#[test]
fn too_long_sequence_is_ignored_and_the_next_sequence_arrives() {
    let mut output = b"\x1b]1337;".to_vec();
    output.extend(std::iter::repeat_n(b'x', 5000));
    output.extend(b"\x07\x1b]133;A\x07");

    assert_eq!(unhandled_osc(&[&output]), [(133, b"A".to_vec())]);
}
