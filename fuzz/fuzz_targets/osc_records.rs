//! Arbitrary payloads go directly into the OSC parsers of `nomux_term::osc`.
//!
//! `terminal_feed` gives the parsers only what the terminal lets through: a
//! payload of 4096 bytes at most that has no byte that ends a sequence. It
//! also examines no record. This target gives each payload to a parser, and
//! a record that a parser returns must obey the limits that the daemon and
//! the clients rely on: each text is one line that is not longer than its
//! limit, and each field has a value that its documentation permits.
//!
//! The first byte of the input selects the code: `0` is 7501, `1` is 133,
//! `2` is 9 and `3` is 777, and each other byte selects as its value modulo
//! 4 does. The rest is the payload. Thus a seed is a line of text.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nomux_term::Osc;
use nomux_term::osc::{
    Notification, OscRecord, ProgramStatusReport, ReportedState, notification, program_status,
};

const CODES: [u32; 4] = [7501, 133, 9, 777];

const MAX_ID_NAMES: usize = 8;
const MAX_ID_NAME_BYTES: usize = 32;

fuzz_target!(|input: &[u8]| {
    let Some((selector, payload)) = input.split_first() else {
        return;
    };
    let code = CODES[usize::from(*selector) % CODES.len()];
    match (Osc { code, payload }).record() {
        None => {}
        Some(OscRecord::ProgramStatusQuery) => assert_eq!((code, payload), (7501, &b"?"[..])),
        Some(OscRecord::ProgramStatus(report)) => {
            assert_eq!(code, 7501);
            assert_report_obeys_limits(&report);
        }
        Some(OscRecord::PromptMark(_)) => assert_eq!(code, 133),
        Some(OscRecord::Notification(notification)) => {
            assert!(code == 9 || code == 777);
            assert_eq!(
                notification.title.is_some(),
                code == 777 && has_title(payload)
            );
            assert_notification_obeys_limits(&notification);
        }
    }
});

fn assert_report_obeys_limits(report: &ProgramStatusReport) {
    assert!(report.id.len() <= program_status::MAX_ID_BYTES);
    if !report.id.is_empty() {
        let names: Vec<&str> = report.id.split('/').collect();
        assert!(names.len() <= MAX_ID_NAMES, "{:?}", report.id);
        for name in names {
            assert!(is_name(name, MAX_ID_NAME_BYTES), "{:?}", report.id);
        }
    }
    let blocked = report.state == ReportedState::Blocked;
    let busy = blocked || report.state == ReportedState::Working;
    assert!(report.kind.is_none() || blocked, "{report:?}");
    assert!(report.progress.is_none() || busy, "{report:?}");
    assert!(report.progress.is_none_or(|progress| progress <= 100));
    if let Some(app) = &report.app {
        assert!(is_name(app, program_status::MAX_APP_BYTES), "{app:?}");
    }
    if let Some(title) = &report.title {
        assert_is_line(title, 1, program_status::MAX_TITLE_BYTES);
    }
    if let Some(message) = &report.message {
        assert_is_line(message, 1, program_status::MAX_MESSAGE_BYTES);
    }
}

fn assert_notification_obeys_limits(notification: &Notification) {
    if let Some(title) = &notification.title {
        assert_is_line(title, 1, notification::MAX_TITLE_BYTES);
    }
    let least = usize::from(notification.title.is_none());
    assert_is_line(&notification.body, least, notification::MAX_BODY_BYTES);
}

/// `true` if the title field of an OSC 777 payload is not empty.
fn has_title(payload: &[u8]) -> bool {
    payload
        .split(|byte| *byte == b';')
        .nth(1)
        .is_some_and(|title| !title.is_empty())
}

/// A name of the Program Status Protocol.
fn is_name(text: &str, max_bytes: usize) -> bool {
    let is_name_byte = |byte: u8| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte);
    (1..=max_bytes).contains(&text.len()) && text.bytes().all(is_name_byte)
}

/// A line feed or an escape sequence in a text can change what a client
/// shows.
fn assert_is_line(text: &str, least_bytes: usize, max_bytes: usize) {
    assert!((least_bytes..=max_bytes).contains(&text.len()), "{text:?}");
    assert!(!text.chars().any(char::is_control), "{text:?}");
}
