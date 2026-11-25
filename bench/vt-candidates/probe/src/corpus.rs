//! Deterministic terminal output for the throughput measurement.
//!
//! Each corpus is generated from a fixed seed, thus two runs and two
//! candidates get the same bytes and the repository has no corpus files.

use std::fmt::Write as _;

const WORDS: &[&str] = &[
    "session",
    "holder",
    "daemon",
    "client",
    "terminal",
    "output",
    "the",
    "of",
    "a",
    "error",
    "warning",
    "compile",
    "finished",
    "running",
    "target",
    "release",
    "src/main.rs",
    "ok",
    "test",
    "0.42s",
    "->",
    "=>",
    "{",
    "}",
    "fn",
    "let",
    "match",
    "return",
];
const WIDE_WORDS: &[&str] = &[
    "日本語",
    "テキスト",
    "中文字符",
    "한국어",
    "données",
    "größe",
    "naïve",
    "e\u{301}te\u{301}",
    "🙂",
    "🚀",
    "✅",
    "│",
    "─┼─",
    "▁▂▃▄▅▆▇█",
    "λ→∀",
];
const SCREEN_ROWS: u64 = 24;
const SCREEN_COLUMNS: u64 = 80;
const LINE_COLUMNS: usize = 100;

/// One kind of terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corpus {
    /// Lines of words, as a build log or `cat` of a text file.
    PlainText,
    /// Lines in which each word has its own SGR attributes and colors.
    SgrColor,
    /// Cursor addressing, erase and line operations, as a full-screen program.
    CursorMotion,
    /// Wide characters, combining marks, emoji and box drawing.
    Unicode,
    /// Very short lines: the screen scrolls for almost each byte.
    ScrollFlood,
}

impl Corpus {
    /// All corpora, in the sequence of the report.
    pub const ALL: [Self; 5] = [
        Self::PlainText,
        Self::SgrColor,
        Self::CursorMotion,
        Self::Unicode,
        Self::ScrollFlood,
    ];

    /// The name of the corpus on the command line of a probe.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::PlainText => "plain-text",
            Self::SgrColor => "sgr-color",
            Self::CursorMotion => "cursor-motion",
            Self::Unicode => "unicode",
            Self::ScrollFlood => "scroll-flood",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|corpus| corpus.name() == name)
    }

    /// Generates at least `bytes` bytes that end at the end of a unit (a line
    /// or a frame), thus no escape sequence is cut.
    pub(crate) fn generate(self, bytes: usize) -> Vec<u8> {
        let mut random = Random::new();
        let mut output = String::with_capacity(bytes + 4096);
        let mut unit = 0;
        while output.len() < bytes {
            match self {
                Self::PlainText => plain_text_line(&mut output, &mut random),
                Self::SgrColor => sgr_color_line(&mut output, &mut random),
                Self::CursorMotion => cursor_motion_frame(&mut output, &mut random),
                Self::Unicode => unicode_line(&mut output, &mut random),
                Self::ScrollFlood => scroll_flood_line(&mut output, unit),
            }
            unit += 1;
        }
        output.into_bytes()
    }
}

/// A linear congruential generator (the MMIX constants of Knuth).
struct Random(u64);

impl Random {
    fn new() -> Self {
        Self(0x6e6f_6d75_785f_7674)
    }

    /// A number in `0..bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        let count = u64::try_from(items.len()).unwrap_or(u64::MAX);
        let index = usize::try_from(self.below(count)).unwrap_or(0);
        items[index]
    }
}

fn plain_text_line(output: &mut String, random: &mut Random) {
    let start = output.len();
    while output.len() - start < LINE_COLUMNS {
        output.push_str(random.pick(WORDS));
        output.push(' ');
    }
    output.push_str("\r\n");
}

fn sgr_color_line(output: &mut String, random: &mut Random) {
    let start = output.len();
    while output.len() - start < 3 * LINE_COLUMNS {
        match random.below(4) {
            0 => write_true_color(output, random),
            1 => write!(output, "\x1b[38;5;{}m", random.below(256)),
            2 => write!(output, "\x1b[1;3{};4{}m", random.below(8), random.below(8)),
            _ => write!(output, "\x1b[0;{}m", random.below(8) + 90),
        }
        .unwrap_or_default();
        output.push_str(random.pick(WORDS));
        output.push(' ');
    }
    output.push_str("\x1b[0m\r\n");
}

fn write_true_color(output: &mut String, random: &mut Random) -> std::fmt::Result {
    let (red, green, blue) = (random.below(256), random.below(256), random.below(256));
    write!(output, "\x1b[38;2;{red};{green};{blue}m")
}

/// One screen update of a full-screen program: status line, some changed
/// cells, a scrolled region and the cursor back in the text.
fn cursor_motion_frame(output: &mut String, random: &mut Random) {
    output.push_str("\x1b[?25l\x1b[1;1H\x1b[7m status \x1b[0m\x1b[K");
    for _ in 0..40 {
        let row = random.below(SCREEN_ROWS - 2) + 2;
        let column = random.below(SCREEN_COLUMNS - 10) + 1;
        write!(output, "\x1b[{row};{column}H\x1b[3{}m", random.below(8)).unwrap_or_default();
        output.push_str(random.pick(WORDS));
        output.push_str("\x1b[0m\x1b[K");
    }
    let row = random.below(SCREEN_ROWS - 4) + 2;
    write!(output, "\x1b[2;{}r\x1b[{row};1H", SCREEN_ROWS - 1).unwrap_or_default();
    output.push_str(if random.below(2) == 0 {
        "\x1b[L"
    } else {
        "\x1b[M"
    });
    write!(
        output,
        "\x1b[r\x1b[{row};{}H\x1b[?25h",
        random.below(40) + 1
    )
    .unwrap_or_default();
}

fn unicode_line(output: &mut String, random: &mut Random) {
    for _ in 0..12 {
        output.push_str(random.pick(WIDE_WORDS));
        output.push(' ');
        output.push_str(random.pick(WORDS));
        output.push(' ');
    }
    output.push_str("\r\n");
}

fn scroll_flood_line(output: &mut String, line: u64) {
    write!(output, "{}\r\n", line % 1000).unwrap_or_default();
}
