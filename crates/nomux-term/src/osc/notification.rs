//! OSC 9 and OSC 777: a program asks the terminal to show a notification.
//!
//! ```text
//! OSC 9 ; body ST                    the form of iTerm2
//! OSC 777 ; notify ; title ; body ST the form of rxvt-unicode
//! ```
//!
//! `ConEmu` uses OSC 9 for commands: `OSC 9 ; number ; ...` with a number
//! from 1 to 12, for example `9;4` for a progress report. Such a sequence is
//! not a notification. Thus a notification whose body is one of these
//! numbers, or starts with one of them and a `;`, is not read.

use super::line_text::line_text;

/// The largest number of bytes of a title.
pub const MAX_TITLE_BYTES: usize = 192;
/// The largest number of bytes of a body.
pub const MAX_BODY_BYTES: usize = 2048;

const LAST_CONEMU_COMMAND: u8 = 12;
const NOTIFY_COMMAND: &[u8] = b"notify";

/// One notification: text of one line each for the title and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The title. OSC 9 has none.
    pub title: Option<String>,
    /// The body. Empty if an OSC 777 notification has only a title.
    pub body: String,
}

/// Parses the payload of OSC 9: the bytes after `9;`.
///
/// Returns `None` if the payload is empty, is a `ConEmu` command, is longer
/// than [`MAX_BODY_BYTES`], or is not one line of UTF-8 text.
#[must_use]
pub fn parse_osc_9(payload: &[u8]) -> Option<Notification> {
    if payload.is_empty() || is_conemu_command(payload) {
        return None;
    }
    Some(Notification {
        title: None,
        body: line_text(payload, MAX_BODY_BYTES)?,
    })
}

/// Parses the payload of OSC 777: the bytes after `777;`.
///
/// Returns `None` if the command is not `notify`, if the title and the body
/// are empty, if one of them is longer than its limit, or if one of them is
/// not one line of UTF-8 text.
#[must_use]
pub fn parse_osc_777(payload: &[u8]) -> Option<Notification> {
    let mut fields = payload.splitn(3, |byte| *byte == b';');
    if fields.next()? != NOTIFY_COMMAND {
        return None;
    }
    let title = line_text(fields.next()?, MAX_TITLE_BYTES)?;
    let body = line_text(fields.next().unwrap_or_default(), MAX_BODY_BYTES)?;
    if title.is_empty() && body.is_empty() {
        return None;
    }
    Some(Notification {
        title: (!title.is_empty()).then_some(title),
        body,
    })
}

/// `true` if the first field of `payload` is a number from 1 to 12.
fn is_conemu_command(payload: &[u8]) -> bool {
    let first_field = payload
        .split(|byte| *byte == b';')
        .next()
        .unwrap_or_default();
    std::str::from_utf8(first_field)
        .ok()
        .filter(|field| field.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|field| field.parse::<u8>().ok())
        .is_some_and(|command| (1..=LAST_CONEMU_COMMAND).contains(&command))
}

#[cfg(test)]
mod tests {
    use super::{MAX_BODY_BYTES, MAX_TITLE_BYTES, Notification, parse_osc_9, parse_osc_777};

    fn notification(title: Option<&str>, body: &str) -> Option<Notification> {
        let title = title.map(str::to_owned);
        let body = body.to_owned();
        (title.is_some() || !body.is_empty()).then_some(Notification { title, body })
    }

    #[test]
    fn osc_9_gives_the_body_with_its_semicolons() {
        assert_eq!(
            parse_osc_9("Build finished; 3 warnings".as_bytes()),
            notification(None, "Build finished; 3 warnings")
        );
        assert_eq!(
            parse_osc_9(b"12 tests failed"),
            notification(None, "12 tests failed")
        );
        assert_eq!(parse_osc_9(b"13;later"), notification(None, "13;later"));
    }

    #[test]
    fn a_conemu_command_is_not_a_notification() {
        for payload in [&b"4;1;50"[..], b"4", b"1;200", b"12", b"9;/home"] {
            assert_eq!(parse_osc_9(payload), None, "{payload:?}");
        }
    }

    #[test]
    fn osc_777_gives_the_title_and_the_body() {
        assert_eq!(
            parse_osc_777(b"notify;Deploy;eu-west; ready"),
            notification(Some("Deploy"), "eu-west; ready")
        );
        assert_eq!(
            parse_osc_777(b"notify;Deploy"),
            notification(Some("Deploy"), "")
        );
        assert_eq!(
            parse_osc_777(b"notify;;only a body"),
            notification(None, "only a body")
        );
    }

    #[test]
    fn osc_777_with_a_different_command_or_without_text_is_no_notification() {
        for payload in [
            &b""[..],
            b"notify",
            b"notify;",
            b"notify;;",
            b"precmd;x;y",
            b"Notify;a;b",
        ] {
            assert_eq!(parse_osc_777(payload), None, "{payload:?}");
        }
    }

    #[test]
    fn text_above_its_limit_or_not_one_line_of_utf8_is_no_notification() {
        let long_body = "b".repeat(MAX_BODY_BYTES + 1);
        let long_title = format!("notify;{};body", "t".repeat(MAX_TITLE_BYTES + 1));

        assert_eq!(parse_osc_9(long_body.as_bytes()), None);
        assert!(parse_osc_9(&long_body.as_bytes()[1..]).is_some());
        assert_eq!(parse_osc_777(long_title.as_bytes()), None);
        assert_eq!(
            parse_osc_777(format!("notify;title;{long_body}").as_bytes()),
            None
        );
        assert_eq!(parse_osc_9(b"bad \xff utf-8"), None);
        assert_eq!(parse_osc_9(b"two\nlines"), None);
        assert_eq!(parse_osc_777(b"notify;ti\x1b[1mtle;body"), None);
        assert_eq!(parse_osc_777(b"notify;title;bo\xc2\x9bdy"), None);
    }
}
