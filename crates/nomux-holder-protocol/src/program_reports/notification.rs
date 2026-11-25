//! One notification that a program asked the terminal to show (OSC 9 or
//! OSC 777).

use super::optional_text;
use crate::wire::DecodeError;
use crate::wire::payload::{PayloadReader, write_block, write_text};

/// One notification. The title and the body are one line each, without
/// control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The title. OSC 9 has none.
    pub title: Option<String>,
    /// The body. It can be empty if the notification has a title.
    pub body: String,
    /// The revision of the program reports when the notification arrived.
    /// It tells a notification from the ones that the daemon already has.
    pub revision: u64,
}

impl Notification {
    pub(super) fn encode(&self, output: &mut Vec<u8>) {
        write_block(output, |block| {
            block.extend_from_slice(&self.revision.to_le_bytes());
            write_text(block, self.title.as_deref().unwrap_or(""));
            write_text(block, &self.body);
        });
    }

    pub(super) fn decode(block: &mut PayloadReader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            revision: block.u64()?,
            title: optional_text(block.text()?),
            body: block.text()?,
        })
    }
}
