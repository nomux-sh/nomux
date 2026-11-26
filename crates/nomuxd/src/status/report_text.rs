//! The rule for a text that a program or an extension reports: one short
//! line.

use super::holder_sources::MAX_MESSAGE_LENGTH;

/// `text` as a part of a status: `None` if it is empty.
///
/// # Errors
///
/// Returns the reason if `text` is longer than [`MAX_MESSAGE_LENGTH`] bytes
/// or is not one line of text. `name` names the text in the reason.
pub(crate) fn one_line(name: &str, text: &str) -> Result<Option<String>, String> {
    if text.len() > MAX_MESSAGE_LENGTH {
        return Err(format!(
            "the {name} has {} bytes: {MAX_MESSAGE_LENGTH} at most",
            text.len()
        ));
    }
    if text.chars().any(char::is_control) {
        return Err(format!(
            "the {name} must be one line of text without a control character"
        ));
    }
    Ok((!text.is_empty()).then(|| text.to_owned()))
}
