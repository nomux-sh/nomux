//! Removes the queries that the terminal of the session answers from output
//! that goes to a second terminal.
//!
//! The holder answers each query of a program from the terminal state of
//! the session. A client that writes the output of the session to a real
//! terminal, for example the CLI in an attach, gives that terminal the same
//! queries. That terminal answers them too, and the client sends its
//! answers to the program as keys: the program gets two answers. The client
//! removes the queries with this filter, thus the second terminal does not
//! see them.
//!
//! The filter removes the queries that [`Terminal`](crate::Terminal)
//! answers, and passes all other bytes in the same order. A query that the
//! terminal does not answer goes to the second terminal, which can answer
//! it.

use crate::sequence_scanner::state::{BELL, ESCAPE, State};

/// A sequence that is longer is not a query: its bytes pass without a
/// delay, and the filter uses a fixed quantity of memory.
const MAX_QUERY_BYTES: usize = 256;

/// A filter for a stream of output. A sequence can start in one call of
/// [`filter`](Self::filter) and end in a later call: the filter keeps its
/// bytes until the end of the sequence.
#[derive(Debug)]
pub struct QueryFilter {
    state: State,
    /// The bytes of the sequence in progress, from its escape character,
    /// while the sequence can be a query.
    held: Vec<u8>,
    /// The sequence in progress is not a query: its bytes pass.
    passing: bool,
    /// The filter removed a string query that an escape character ended:
    /// the `ESC \` that follows is its end and goes too.
    removed_string: bool,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            state: State::Ground,
            held: Vec::new(),
            passing: false,
            removed_string: false,
        }
    }
}

impl QueryFilter {
    /// Appends to `output` the bytes of `bytes` that are not a part of a
    /// query.
    pub fn filter(&mut self, bytes: &[u8], output: &mut Vec<u8>) {
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.state == State::Ground {
                let Some(index) = memchr::memchr(ESCAPE, rest) else {
                    output.extend_from_slice(rest);
                    return;
                };
                output.extend_from_slice(&rest[..index]);
                rest = &rest[index..];
            }
            self.take(rest[0], output);
            rest = &rest[1..];
        }
    }

    fn take(&mut self, byte: u8, output: &mut Vec<u8>) {
        let before = self.state;
        if byte == ESCAPE {
            if before != State::Ground {
                self.end_at_escape(before, output);
            }
            self.start_sequence();
            return;
        }
        self.state = before.after(byte);
        if self.passing {
            output.push(byte);
            if self.state == State::Ground {
                self.passing = false;
            }
            return;
        }
        if is_control(byte) && executes_controls(before) {
            // The terminal executes the control character immediately, in
            // the sequence: it goes before the bytes of the sequence.
            output.push(byte);
            return;
        }
        self.held.push(byte);
        if self.state == State::Ground {
            self.end_sequence(output);
        } else if self.held.len() > MAX_QUERY_BYTES {
            output.append(&mut self.held);
            self.passing = true;
        }
    }

    fn start_sequence(&mut self) {
        self.held.clear();
        self.held.push(ESCAPE);
        self.passing = false;
        self.state = State::Escape;
    }

    /// An escape character ends an OSC or a DCS sequence, and it cancels
    /// any other sequence.
    fn end_at_escape(&mut self, before: State, output: &mut Vec<u8>) {
        self.removed_string = false;
        if self.passing {
            return;
        }
        let ends_string = matches!(before, State::Osc | State::DcsPassthrough);
        if ends_string && is_query(&self.held) {
            self.removed_string = true;
        } else {
            output.extend_from_slice(&self.held);
        }
    }

    fn end_sequence(&mut self, output: &mut Vec<u8>) {
        let is_end_of_removed_string =
            std::mem::take(&mut self.removed_string) && self.held == b"\x1b\\";
        if !is_end_of_removed_string && !is_query(&self.held) {
            output.extend_from_slice(&self.held);
        }
        self.held.clear();
    }
}

/// A control character that is not a part of the sequence. The escape
/// character, CAN and SUB change the state.
fn is_control(byte: u8) -> bool {
    matches!(byte, 0x00..=0x17 | 0x19 | 0x1c..=0x1f)
}

/// The states in which the terminal executes a control character.
fn executes_controls(state: State) -> bool {
    matches!(
        state,
        State::Escape | State::EscapeIntermediate | State::Csi
    )
}

/// `true` if `sequence`, from its escape character to its end, is a query
/// that the terminal answers.
fn is_query(sequence: &[u8]) -> bool {
    match sequence.get(1) {
        Some(b'[') => is_csi_query(&sequence[2..]),
        Some(b']') => is_color_query(&sequence[2..]),
        Some(b'P') => sequence[2..].starts_with(b"$q"),
        _ => false,
    }
}

/// The queries of a CSI sequence that the terminal answers. `body` has the
/// parameter bytes, the intermediate bytes and the final byte.
fn is_csi_query(body: &[u8]) -> bool {
    let Some((final_byte, parameters)) = body.split_last() else {
        return false;
    };
    let is_zero = |digits: &[u8]| digits.iter().all(|digit| *digit == b'0');
    match final_byte {
        // Device attributes, primary and secondary.
        b'c' => is_zero(parameters.strip_prefix(b">").unwrap_or(parameters)),
        // Device status and cursor position.
        b'n' => matches!(number(parameters), Some(5 | 6)),
        // Mode request (DECRQM), ANSI and DEC private modes.
        b'p' => parameters.ends_with(b"$"),
        // Kitty keyboard flags.
        b'u' => parameters == b"?",
        // Text area size in pixels and in cells.
        b't' => matches!(number(parameters), Some(14 | 18)),
        // Terminal name and version (XTVERSION).
        b'q' => parameters.strip_prefix(b">").is_some_and(is_zero),
        _ => false,
    }
}

/// OSC 4, 10, 11 and 12 with a `?` in the place of a color. `body` has the
/// data of the sequence, and the BEL that ends it if a BEL ends it.
fn is_color_query(body: &[u8]) -> bool {
    let body = body.strip_suffix(&[BELL]).unwrap_or(body);
    let mut fields = body.split(|byte| *byte == b';');
    let code = fields.next().unwrap_or_default();
    matches!(code, b"4" | b"10" | b"11" | b"12") && fields.any(|field| field == b"?")
}

fn number(digits: &[u8]) -> Option<u32> {
    std::str::from_utf8(digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::QueryFilter;

    fn filtered(chunks: &[&[u8]]) -> Vec<u8> {
        let mut filter = QueryFilter::default();
        let mut output = Vec::new();
        for chunk in chunks {
            filter.filter(chunk, &mut output);
        }
        output
    }

    fn assert_filtered(input: &[u8], expected: &[u8]) {
        assert_eq!(
            String::from_utf8_lossy(&filtered(&[input])),
            String::from_utf8_lossy(expected)
        );
        let one_byte_each: Vec<&[u8]> = input.chunks(1).collect();
        assert_eq!(
            String::from_utf8_lossy(&filtered(&one_byte_each)),
            String::from_utf8_lossy(expected),
            "one byte in each call"
        );
    }

    #[test]
    fn each_query_that_the_terminal_answers_goes() {
        let queries: &[&[u8]] = &[
            b"\x1b[c",
            b"\x1b[0c",
            b"\x1b[>c",
            b"\x1b[>0c",
            b"\x1b[5n",
            b"\x1b[6n",
            b"\x1b[?2004$p",
            b"\x1b[4$p",
            b"\x1b[?u",
            b"\x1b[14t",
            b"\x1b[18t",
            b"\x1b[>q",
            b"\x1b]10;?\x07",
            b"\x1b]11;?\x1b\\",
            b"\x1b]12;?\x07",
            b"\x1b]4;1;?;2;?\x07",
            b"\x1bP$qm\x1b\\",
        ];
        for query in queries {
            let input = [b"a", *query, b"b"].concat();
            assert_filtered(&input, b"ab");
        }
    }

    #[test]
    fn the_other_sequences_pass_in_their_order() {
        let others: &[&[u8]] = &[
            b"\x1b[1;31m",
            b"\x1b[=c",
            b"\x1b[1c",
            b"\x1b[?6n",
            b"\x1b[>4;1m",
            b"\x1b[>1u",
            b"\x1b[8;24;80t",
            b"\x1b[2 q",
            b"\x1b]11;#000000\x07",
            b"\x1b]52;c;?\x07",
            b"\x1b]7501;?\x07",
            b"\x1b]0;title\x1b\\",
            b"\x1bP+q544e\x1b\\",
            b"\x1b7",
        ];
        for other in others {
            let input = [b"a", *other, b"b"].concat();
            assert_filtered(&input, &input);
        }
    }

    #[test]
    fn a_control_character_in_a_query_stays_and_a_cancelled_sequence_passes() {
        assert_filtered(b"\x1b[\r6n", b"\r");
        assert_filtered(b"\x1b[6\x18n", b"\x1b[6\x18n");
        assert_filtered(b"\x1b[6\x1b[c", b"\x1b[6");
    }

    #[test]
    fn a_long_sequence_passes_and_the_next_query_goes() {
        let long = [b"\x1b]52;c;".as_slice(), &[b'A'; 1000], b"\x07"].concat();
        let input = [long.as_slice(), b"\x1b[c", b"x"].concat();
        assert_filtered(&input, &[long.as_slice(), b"x"].concat());
    }
}
