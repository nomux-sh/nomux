//! Text of one line that a program sent for a person to read.

/// Returns `bytes` as text. Returns `None` if `bytes` is longer than
/// `max_bytes`, is not UTF-8, or has a control character (U+0000 to U+001F,
/// U+007F to U+009F): a line feed or an escape sequence in the text can
/// change what a client shows.
pub(super) fn line_text(bytes: &[u8], max_bytes: usize) -> Option<String> {
    if bytes.len() > max_bytes {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    (!text.chars().any(char::is_control)).then(|| text.to_owned())
}
