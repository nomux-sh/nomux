//! Conformance cases: the sequences that full-screen programs and agent TUIs
//! send, with the screen text and the cursor position that a correct terminal
//! has after them.

use crate::candidate::{Position, Screen, Size};

/// A small screen keeps the wrap and scroll cases short.
const SIZE: Size = Size {
    rows: 6,
    columns: 20,
};
const SIX_LINES: &str = "L1\r\nL2\r\nL3\r\nL4\r\nL5\r\nL6";

struct Case {
    name: &'static str,
    input: String,
    /// The expected text of the rows from the top, without trailing blanks.
    rows: &'static [&'static str],
    /// The expected cursor position as (row, column).
    cursor: Option<(u16, u16)>,
}

impl Case {
    fn new(name: &'static str, input: impl Into<String>, rows: &'static [&'static str]) -> Self {
        Self {
            name,
            input: input.into(),
            rows,
            cursor: None,
        }
    }

    fn cursor(mut self, row: u16, column: u16) -> Self {
        self.cursor = Some((row, column));
        self
    }

    /// Returns the difference from the expectation, if there is one.
    fn failure<S: Screen>(&self) -> Option<String> {
        let mut screen = S::new(SIZE, 0);
        screen.feed(self.input.as_bytes());
        for (row, expected) in (0..).zip(self.rows) {
            let text = screen.row_text(row);
            if text.trim_end() != *expected {
                return Some(format!("row {row} is {:?}", text.trim_end()));
            }
        }
        let cursor = screen.cursor();
        let expected = self.cursor.map(|(row, column)| Position { row, column });
        if expected.is_some_and(|expected| expected != cursor) {
            return Some(format!("cursor is at {},{}", cursor.row, cursor.column));
        }
        None
    }
}

/// Prints one line for each case: `pass` or `fail`, the case name and, for a
/// failure, the difference.
pub(crate) fn report<S: Screen>() {
    for case in cases() {
        match case.failure::<S>() {
            None => println!("pass\t{}", case.name),
            Some(difference) => println!("fail\t{}\t{difference}", case.name),
        }
    }
}

fn cases() -> Vec<Case> {
    let mut cases = cursor_cases();
    cases.extend(wrap_cases());
    cases.extend(erase_and_edit_cases());
    cases.extend(scroll_cases());
    cases.extend(screen_and_charset_cases());
    cases.extend(ignored_sequence_cases());
    cases
}

fn cursor_cases() -> Vec<Case> {
    vec![
        Case::new("cursor position (CUP)", "\x1b[3;5HX", &["", "", "    X"]).cursor(2, 5),
        Case::new("cursor position is clamped", "\x1b[99;99H", &[]).cursor(5, 19),
        Case::new(
            "line and column absolute (VPA, CHA)",
            "\x1b[3d\x1b[5GX",
            &["", "", "    X"],
        ),
        Case::new(
            "cursor next and previous line (CNL, CPL)",
            "ab\x1b[2Ec\x1b[1Fd",
            &["ab", "d", "c"],
        ),
        Case::new(
            "save and restore cursor (DECSC, DECRC)",
            "\x1b[2;3H\x1b7\x1b[5;5H\x1b8X",
            &["", "  X"],
        ),
        Case::new(
            "save and restore cursor (CSI s, CSI u)",
            "\x1b[2;3H\x1b[s\x1b[5;5H\x1b[uX",
            &["", "  X"],
        ),
        Case::new("default tab stops", "a\tb", &["a       b"]),
        Case::new("set tab stop (HTS)", "\x1b[1;5H\x1bH\r\tx", &["    x"]),
        Case::new("backward tab (CBT)", "\x1b[1;12H\x1b[Zx", &["        x"]),
        Case::new(
            "index and next line (IND, NEL)",
            "a\x1bDb\x1bEc",
            &["a", " b", "c"],
        ),
        Case::new(
            "origin mode (DECOM)",
            "\x1b[2;4r\x1b[?6h\x1b[1;1HX",
            &["", "X"],
        ),
    ]
}

fn wrap_cases() -> Vec<Case> {
    vec![
        Case::new(
            "autowrap",
            "a".repeat(25),
            &["aaaaaaaaaaaaaaaaaaaa", "aaaaa"],
        )
        .cursor(1, 5),
        Case::new(
            "no empty line after a full line",
            format!("{}\r\nx", "a".repeat(20)),
            &["aaaaaaaaaaaaaaaaaaaa", "x"],
        )
        .cursor(1, 1),
        Case::new(
            "autowrap off (DECAWM)",
            format!("\x1b[?7l{}z", "a".repeat(24)),
            &["aaaaaaaaaaaaaaaaaaaz", ""],
        ),
        Case::new("wide characters", "日本x", &["日本x"]).cursor(0, 5),
        Case::new(
            "wide character wraps at the last column",
            format!("{}日", "a".repeat(19)),
            &["aaaaaaaaaaaaaaaaaaa", "日"],
        )
        .cursor(1, 2),
        Case::new("combining mark", "e\u{301}x", &["e\u{301}x"]).cursor(0, 2),
        Case::new("emoji", "🚀x", &["🚀x"]).cursor(0, 3),
    ]
}

fn erase_and_edit_cases() -> Vec<Case> {
    vec![
        Case::new("erase to end of line (EL)", "abcdef\x1b[3G\x1b[K", &["ab"]),
        Case::new(
            "erase to start of line (EL 1)",
            "abcdef\x1b[3G\x1b[1K",
            &["   def"],
        ),
        Case::new(
            "erase below (ED)",
            format!("{SIX_LINES}\x1b[2;2H\x1b[J"),
            &["L1", "L", "", ""],
        ),
        Case::new(
            "erase display keeps the cursor (ED 2)",
            "ab\r\ncd\x1b[2JX",
            &["", "  X"],
        ),
        Case::new(
            "erase characters (ECH)",
            "abcdef\x1b[1;2H\x1b[3X",
            &["a   ef"],
        ),
        Case::new(
            "insert characters (ICH)",
            "abcdef\x1b[1;3H\x1b[2@",
            &["ab  cdef"],
        ),
        Case::new(
            "delete characters (DCH)",
            "abcdef\x1b[1;3H\x1b[2P",
            &["abef"],
        ),
        Case::new("insert mode (IRM)", "abc\x1b[1;1H\x1b[4hX", &["Xabc"]),
        Case::new("repeat character (REP)", "a\x1b[3b", &["aaaa"]),
        Case::new(
            "insert line (IL)",
            "1\r\n2\r\n3\x1b[2;1H\x1b[L",
            &["1", "", "2", "3"],
        ),
        Case::new(
            "delete line (DL)",
            "1\r\n2\r\n3\x1b[2;1H\x1b[M",
            &["1", "3", ""],
        ),
    ]
}

fn scroll_cases() -> Vec<Case> {
    vec![
        Case::new(
            "scroll at the bottom",
            format!("{SIX_LINES}\r\nL7"),
            &["L2", "L3", "L4", "L5", "L6", "L7"],
        ),
        Case::new(
            "scroll region (DECSTBM)",
            format!("{SIX_LINES}\x1b[2;4r\x1b[4;1H\n"),
            &["L1", "L3", "L4", "", "L5", "L6"],
        ),
        Case::new(
            "reverse index in a scroll region (RI)",
            format!("{SIX_LINES}\x1b[2;4r\x1b[2;1H\x1bM"),
            &["L1", "", "L2", "L3", "L5", "L6"],
        ),
        Case::new(
            "scroll up (SU)",
            format!("{SIX_LINES}\x1b[2S"),
            &["L3", "L4", "L5", "L6", "", ""],
        ),
        Case::new(
            "scroll down (SD)",
            format!("{SIX_LINES}\x1b[2T"),
            &["", "", "L1", "L2", "L3", "L4"],
        ),
    ]
}

fn screen_and_charset_cases() -> Vec<Case> {
    vec![
        Case::new(
            "alternate screen (1049) keeps the cursor position",
            "main\x1b[?1049halt",
            &["    alt"],
        ),
        Case::new(
            "alternate screen (1049) restores the screen",
            "main\x1b[?1049halt\x1b[?1049l",
            &["main"],
        )
        .cursor(0, 4),
        Case::new(
            "alternate screen (1047) restores the screen",
            "main\x1b[?1047h\x1b[Halt\x1b[?1047l",
            &["main"],
        ),
        Case::new("full reset (RIS)", "abc\r\ndef\x1bc", &["", ""]).cursor(0, 0),
        Case::new(
            "DEC line drawing character set",
            "\x1b(0lqk\x1b(Bx",
            &["┌─┐x"],
        ),
    ]
}

/// Sequences that change no cell. A terminal that does not know one of them
/// must not print a part of it.
fn ignored_sequence_cases() -> Vec<Case> {
    vec![
        Case::new(
            "SGR with colon parameters prints no text",
            "\x1b[4:3;38:2::10:20:30mX\x1b[0m",
            &["X"],
        ),
        Case::new(
            "OSC 8 hyperlink prints only the link text",
            "\x1b]8;;https://nomux.sh\x1b\\link\x1b]8;;\x1b\\",
            &["link"],
        ),
        Case::new(
            "OSC 133 prompt mark prints no text",
            "\x1b]133;A\x07$ \x1b]133;B\x07",
            &["$"],
        ),
        Case::new("DCS request prints no text", "\x1bP+q544e\x1b\\X", &["X"]),
        Case::new(
            "kitty graphics (APC) prints no text",
            "\x1b_Gf=24,s=1,v=1;AAAA\x1b\\X",
            &["X"],
        ),
        Case::new(
            "kitty keyboard push and pop print no text",
            "\x1b[>1uX\x1b[<uY",
            &["XY"],
        ),
        Case::new(
            "synchronized output (2026) prints the text",
            "\x1b[?2026hX\x1b[?2026l",
            &["X"],
        ),
    ]
}
