//! An event of an extension, as a subscriber gets it.

/// One event that an extension sent.
///
/// Two extensions can use the same event name. The identifier of the
/// extension makes the event unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionEvent {
    /// The identifier of the extension that sent the event.
    pub extension_id: String,
    /// The name that the extension gave to the event.
    pub name: String,
    /// The payload. The extension selects its encoding.
    pub payload: Vec<u8>,
}
