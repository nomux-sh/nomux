//! The OSC sequences in the output of a program.
//!
//! The VT library has no hook for the OSC codes that it does not know. The
//! [`SequenceScanner`](crate::sequence_scanner::SequenceScanner) finds the
//! OSC sequences, and the modules here parse the payload of the codes with
//! which a program or a shell reports its state:
//!
//! | Code | Module | Record |
//! |---|---|---|
//! | 7501 | [`program_status`] | [`ProgramStatusReport`], and the query |
//! | 133 | [`prompt_mark`] | [`PromptMark`] |
//! | 9, 777 | [`notification`] | [`Notification`] |
//!
//! A payload is untrusted input. Each parser has a size limit for each
//! field, allocates only the fields of its record, and returns `None` for a
//! payload that is not valid.

pub mod notification;
pub mod program_status;
pub mod prompt_mark;

mod line_text;

pub use notification::Notification;
pub use program_status::{BlockedKind, ProgramStatusReport, ReportedState};
pub use prompt_mark::PromptMark;

const PROGRAM_STATUS: u32 = 7501;
const PROMPT_MARK: u32 = 133;
const NOTIFICATION: u32 = 9;
const EXTENSION_COMMAND: u32 = 777;

/// An operating system command (OSC) from the output of a program:
/// `ESC ] code ; payload`, with BEL or `ESC \` at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Osc<'a> {
    /// The number before the first `;`, for example 133 or 7501.
    pub code: u32,
    /// The bytes after the first `;`. Empty if the sequence has no `;`.
    pub payload: &'a [u8],
}

/// What a program or a shell reported with one OSC sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscRecord {
    /// OSC 7501 with the payload `?`: the program asks if the terminal
    /// reads program status reports. The answer is
    /// [`program_status::QUERY_REPLY`].
    ProgramStatusQuery,
    /// OSC 7501: a program status report.
    ProgramStatus(ProgramStatusReport),
    /// OSC 133: a prompt mark of the shell.
    PromptMark(PromptMark),
    /// OSC 9 or OSC 777: a notification.
    Notification(Notification),
}

impl<'a> Osc<'a> {
    /// Returns `None` if the sequence does not start with a decimal number.
    pub(crate) fn parse(sequence: &'a [u8]) -> Option<Self> {
        let separator = sequence.iter().position(|byte| *byte == b';');
        let (digits, payload) = match separator {
            Some(index) => (&sequence[..index], &sequence[index + 1..]),
            None => (sequence, &[][..]),
        };
        let code = std::str::from_utf8(digits).ok()?.parse().ok()?;
        Some(Self { code, payload })
    }

    /// Parses the payload of the codes that report state. Returns `None`
    /// for a different code and for a payload that is not valid.
    #[must_use]
    pub fn record(&self) -> Option<OscRecord> {
        match self.code {
            PROGRAM_STATUS if program_status::is_query(self.payload) => {
                Some(OscRecord::ProgramStatusQuery)
            }
            PROGRAM_STATUS => program_status::parse(self.payload).map(OscRecord::ProgramStatus),
            PROMPT_MARK => prompt_mark::parse(self.payload).map(OscRecord::PromptMark),
            NOTIFICATION => notification::parse_osc_9(self.payload).map(OscRecord::Notification),
            EXTENSION_COMMAND => {
                notification::parse_osc_777(self.payload).map(OscRecord::Notification)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Osc, OscRecord, PromptMark};

    fn record(code: u32, payload: &[u8]) -> Option<OscRecord> {
        Osc { code, payload }.record()
    }

    #[test]
    fn each_code_goes_to_its_parser() {
        assert_eq!(record(7501, b"?"), Some(OscRecord::ProgramStatusQuery));
        assert!(matches!(
            record(7501, b"state=idle"),
            Some(OscRecord::ProgramStatus(_))
        ));
        assert_eq!(
            record(133, b"B"),
            Some(OscRecord::PromptMark(PromptMark::InputStart))
        );
        let Some(OscRecord::Notification(from_osc_9)) = record(9, b"notify;a;b") else {
            panic!("OSC 9 is a notification");
        };
        assert_eq!(
            (from_osc_9.title, from_osc_9.body.as_str()),
            (None, "notify;a;b")
        );
        let Some(OscRecord::Notification(from_osc_777)) = record(777, b"notify;a;b") else {
            panic!("OSC 777 is a notification");
        };
        assert_eq!(from_osc_777.title.as_deref(), Some("a"));
    }

    #[test]
    fn a_different_code_and_a_payload_that_is_not_valid_give_no_record() {
        assert_eq!(record(99, b"state=idle"), None);
        assert_eq!(record(1337, b"A"), None);
        assert_eq!(record(7501, b"state=later"), None);
        assert_eq!(record(133, b"Z"), None);
    }
}
