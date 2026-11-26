//! A line of the text output that is not a table. On a terminal, it breaks
//! at spaces to fit the width of the terminal, as a table fits it.

use unicode_width::UnicodeWidthStr;

/// `line` (without its line feed) as one or more lines that end in a line
/// feed. With `width`, a line breaks at the last space that fits. A word
/// that is wider than `width` is not cut.
pub(super) fn wrapped(line: &str, width: Option<usize>) -> String {
    let Some(width) = width else {
        return format!("{line}\n");
    };
    let mut text = String::new();
    let mut current = String::new();
    for word in line.split(' ') {
        if !current.is_empty() && current.width() + 1 + word.width() > width {
            text.push_str(&current);
            text.push('\n');
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    text.push_str(&current);
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_breaks_at_the_last_space_that_fits_and_a_long_word_is_not_cut() {
        assert_eq!(wrapped("one two three", Some(7)), "one two\nthree\n");
        assert_eq!(wrapped("a verylongword b", Some(5)), "a\nverylongword\nb\n");
        assert_eq!(wrapped("one two three", None), "one two three\n");
    }
}
