//! The positions of the values of a JSON text. An edit of a settings file
//! changes only the bytes at these positions, thus the rest of the file
//! keeps its bytes.
//!
//! The scanner reads a text that `serde_json` accepted: it does not check
//! each rule of JSON again.

use std::ops::Range;

/// One value of the text, at the bytes `span`.
#[derive(Debug)]
pub(super) struct Node {
    pub(super) span: Range<usize>,
    pub(super) kind: Kind,
}

#[derive(Debug)]
pub(super) enum Kind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    Scalar,
}

/// One member of an object: its key, and the bytes from the start of the
/// key to the end of the value.
#[derive(Debug)]
pub(super) struct Member {
    pub(super) key: String,
    pub(super) span: Range<usize>,
    pub(super) value: Node,
}

/// The root value of `text`. `None` if `text` is not JSON.
pub(super) fn scan(text: &str) -> Option<Node> {
    let mut scanner = Scanner {
        bytes: text.as_bytes(),
        text,
        position: 0,
    };
    let root = scanner.value()?;
    scanner.skip_whitespace();
    (scanner.position == text.len()).then_some(root)
}

struct Scanner<'text> {
    bytes: &'text [u8],
    text: &'text str,
    position: usize,
}

impl Scanner<'_> {
    fn value(&mut self) -> Option<Node> {
        self.skip_whitespace();
        let start = self.position;
        let kind = match self.bytes.get(start)? {
            b'{' => Kind::Object(self.members()?),
            b'[' => Kind::Array(self.items()?),
            b'"' => {
                self.string()?;
                Kind::Scalar
            }
            _ => {
                self.literal();
                Kind::Scalar
            }
        };
        (self.position > start).then_some(Node {
            span: start..self.position,
            kind,
        })
    }

    fn members(&mut self) -> Option<Vec<Member>> {
        self.position += 1;
        let mut members = Vec::new();
        if self.close(b'}') {
            return Some(members);
        }
        loop {
            self.skip_whitespace();
            let start = self.position;
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.value()?;
            members.push(Member {
                key: serde_json::from_str(&self.text[key]).ok()?,
                span: start..value.span.end,
                value,
            });
            if self.close(b'}') {
                return Some(members);
            }
            self.expect(b',')?;
        }
    }

    fn items(&mut self) -> Option<Vec<Node>> {
        self.position += 1;
        let mut items = Vec::new();
        if self.close(b']') {
            return Some(items);
        }
        loop {
            items.push(self.value()?);
            if self.close(b']') {
                return Some(items);
            }
            self.expect(b',')?;
        }
    }

    /// The bytes of a string with its quotes.
    fn string(&mut self) -> Option<Range<usize>> {
        let start = self.position;
        self.expect(b'"')?;
        loop {
            match self.bytes.get(self.position)? {
                b'\\' => self.position += 2,
                b'"' => {
                    self.position += 1;
                    return Some(start..self.position);
                }
                _ => self.position += 1,
            }
        }
    }

    /// A number, `true`, `false` or `null`.
    fn literal(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || b"+-.".contains(byte))
        {
            self.position += 1;
        }
    }

    /// Skips the whitespace and the byte `end` if it comes next.
    fn close(&mut self, end: u8) -> bool {
        self.skip_whitespace();
        let closes = self.bytes.get(self.position) == Some(&end);
        if closes {
            self.position += 1;
        }
        closes
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        self.skip_whitespace();
        (self.bytes.get(self.position) == Some(&byte)).then(|| self.position += 1)
    }

    fn skip_whitespace(&mut self) {
        while self
            .bytes
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.position += 1;
        }
    }
}
