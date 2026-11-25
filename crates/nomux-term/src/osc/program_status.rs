//! OSC 7501, the Program Status Protocol, revision 0.4: a program reports
//! what it does.
//!
//! ```text
//! OSC 7501 ; key=value : key=value ... ST    a report
//! OSC 7501 ; ? ST                            the query
//! ```
//!
//! The specification is at <https://control-codes.page/proposals/osc-7501/>.
//! The rules of this parser are the rules of the specification:
//!
//! - A pair without `=`, a pair with a byte that a value cannot have, and a
//!   key that the parser does not know are ignored. If a key repeats, the
//!   last value is used.
//! - The report is discarded if a key or a value is longer than its limit,
//!   if the `id` is not a path of names, if `title` or `msg` is not base64 of
//!   one line of UTF-8 text, or if `state` is absent or not known.
//! - One rule is more strict than the text of the specification: an `id`
//!   with a byte that a value cannot have discards the report. If only the
//!   pair was ignored, a `clear` for one record removed all records.
//! - `kind` applies only to the state `blocked`, and `progress` only to
//!   `working` and `blocked`. A value that is not valid there is absent.

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use super::line_text::line_text;

/// The answer to the query: the states and the kinds that [`parse`] reads.
/// The terminal writes these bytes to the input of the program.
pub const QUERY_REPLY: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";

/// The largest number of bytes of an `id`.
pub const MAX_ID_BYTES: usize = 128;
/// The largest number of bytes of an `app` value.
pub const MAX_APP_BYTES: usize = 32;
/// The largest number of bytes of a decoded `title`.
pub const MAX_TITLE_BYTES: usize = 192;
/// The largest number of bytes of a decoded `msg`.
pub const MAX_MESSAGE_BYTES: usize = 2048;

/// The specification permits 4096 bytes for the sequence with its start and
/// its end.
const MAX_PAYLOAD_BYTES: usize = 4096 - b"\x1b]7501;".len() - b"\x1b\\".len();
const MAX_KEY_BYTES: usize = 16;
const MAX_ID_SEGMENTS: usize = 8;
const MAX_ID_SEGMENT_BYTES: usize = 32;
const MAX_TITLE_ENCODED_BYTES: usize = 256;
const MAX_MESSAGE_ENCODED_BYTES: usize = 2732;
/// The decoder asks for 3 bytes for each 4 bytes of input.
const DECODE_BUFFER_BYTES: usize = MAX_MESSAGE_ENCODED_BYTES.div_ceil(4) * 3;

/// Standard base64. The padding is optional.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The value of the `state` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedState {
    /// The program waits for the next instruction of the user.
    Idle,
    /// The program runs.
    Working,
    /// The program completed work that the user did not see yet.
    Done,
    /// The program cannot continue until the user does something.
    Blocked,
    /// The program failed and stopped.
    Error,
    /// Not a state: the report removes the record of its `id` and each
    /// record below it. Without an `id`, it removes all records.
    Clear,
}

impl ReportedState {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"idle" => Some(Self::Idle),
            b"working" => Some(Self::Working),
            b"done" => Some(Self::Done),
            b"blocked" => Some(Self::Blocked),
            b"error" => Some(Self::Error),
            b"clear" => Some(Self::Clear),
            _ => None,
        }
    }
}

/// The value of the `kind` key: what a blocked program waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedKind {
    /// The user must approve an operation.
    Permission,
    /// The user must type an answer.
    Question,
    /// The user must give a login, a token or a credential.
    Auth,
}

impl BlockedKind {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"permission" => Some(Self::Permission),
            b"question" => Some(Self::Question),
            b"auth" => Some(Self::Auth),
            _ => None,
        }
    }
}

/// One program status report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusReport {
    /// The record that the report is for: a path of 8 names at most with
    /// `/` between them. Empty for the root record.
    pub id: String,
    /// The value of the `state` key.
    pub state: ReportedState,
    /// What the program waits for. Only with [`ReportedState::Blocked`].
    pub kind: Option<BlockedKind>,
    /// The percentage of the work that is done, 0 to 100. Only with
    /// [`ReportedState::Working`] and [`ReportedState::Blocked`].
    pub progress: Option<u8>,
    /// The stable name of the program, for example `cargo`.
    pub app: Option<String>,
    /// The label of the record: one line.
    pub title: Option<String>,
    /// What the record does, waits for or completed: one line.
    pub message: Option<String>,
}

/// `true` if `payload` is the query.
#[must_use]
pub fn is_query(payload: &[u8]) -> bool {
    payload == b"?"
}

/// Parses the payload of a report: the bytes after `7501;`.
///
/// Returns `None` if the specification says to discard the report.
#[must_use]
pub fn parse(payload: &[u8]) -> Option<ProgramStatusReport> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    let values = LastValues::collect(payload)?;
    let state = ReportedState::parse(values.state?)?;
    let blocked = state == ReportedState::Blocked;
    let busy = blocked || state == ReportedState::Working;
    Some(ProgramStatusReport {
        id: values.id.map_or_else(|| Some(String::new()), parse_id)?,
        state,
        kind: values.kind.filter(|_| blocked).and_then(BlockedKind::parse),
        progress: values.progress.filter(|_| busy).and_then(parse_progress),
        app: values
            .app
            .filter(|app| is_name(app, MAX_APP_BYTES))
            .map(ascii_text),
        title: decode_text(values.title, MAX_TITLE_BYTES).ok()?,
        message: decode_text(values.message, MAX_MESSAGE_BYTES).ok()?,
    })
}

/// The last value of each key that the parser knows.
#[derive(Default)]
struct LastValues<'payload> {
    state: Option<&'payload [u8]>,
    id: Option<&'payload [u8]>,
    kind: Option<&'payload [u8]>,
    progress: Option<&'payload [u8]>,
    app: Option<&'payload [u8]>,
    title: Option<&'payload [u8]>,
    message: Option<&'payload [u8]>,
}

impl<'payload> LastValues<'payload> {
    /// Returns `None` if a key or a value is longer than its limit, or if
    /// an `id` has a byte that a value cannot have. The limit applies also
    /// to a value that a later pair replaces.
    fn collect(payload: &'payload [u8]) -> Option<Self> {
        let mut values = Self::default();
        for pair in payload.split(|byte| *byte == b':') {
            let Some(separator) = pair.iter().position(|byte| *byte == b'=') else {
                continue;
            };
            let key = pair[..separator].trim_ascii();
            let value = pair[separator + 1..].trim_ascii();
            if key.len() > MAX_KEY_BYTES {
                return None;
            }
            if !value.iter().copied().all(is_value_byte) {
                // Without its `id`, the report is for the root record.
                if key == b"id" {
                    return None;
                }
                continue;
            }
            let (slot, max_bytes) = match key {
                b"state" => (&mut values.state, usize::MAX),
                b"id" => (&mut values.id, MAX_ID_BYTES),
                b"kind" => (&mut values.kind, usize::MAX),
                b"progress" => (&mut values.progress, usize::MAX),
                b"app" => (&mut values.app, MAX_APP_BYTES),
                b"title" => (&mut values.title, MAX_TITLE_ENCODED_BYTES),
                b"msg" => (&mut values.message, MAX_MESSAGE_ENCODED_BYTES),
                _ => continue,
            };
            if value.len() > max_bytes {
                return None;
            }
            *slot = Some(value);
        }
        Some(values)
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte)
}

fn is_value_byte(byte: u8) -> bool {
    is_name_byte(byte) || b",/=".contains(&byte)
}

/// A name is the form of an `app` value and of each part of an `id`.
fn is_name(value: &[u8], max_bytes: usize) -> bool {
    (1..=max_bytes).contains(&value.len()) && value.iter().copied().all(is_name_byte)
}

fn ascii_text(value: &[u8]) -> String {
    String::from_utf8_lossy(value).into_owned()
}

/// Returns `None` if `value` is not a path of names. The report is then
/// discarded: if it went to the root record, a `clear` with a bad `id`
/// removed all records.
fn parse_id(value: &[u8]) -> Option<String> {
    let mut segments = value.split(|byte| *byte == b'/');
    let valid = segments.clone().count() <= MAX_ID_SEGMENTS
        && segments.all(|segment| is_name(segment, MAX_ID_SEGMENT_BYTES));
    valid.then(|| ascii_text(value))
}

/// A number from 0 to 100 that has only digits.
fn parse_progress(value: &[u8]) -> Option<u8> {
    if !value.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let progress: u8 = std::str::from_utf8(value).ok()?.parse().ok()?;
    (progress <= 100).then_some(progress)
}

/// A `title` or a `msg` that discards its report: the value is not base64,
/// or the text is not one line of UTF-8, or the text is too long.
struct InvalidText;

/// Decodes a `title` or a `msg`. An empty text is no text.
fn decode_text(encoded: Option<&[u8]>, max_bytes: usize) -> Result<Option<String>, InvalidText> {
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    let mut decoded = [0u8; DECODE_BUFFER_BYTES];
    let length = BASE64
        .decode_slice(encoded, &mut decoded)
        .map_err(|_| InvalidText)?;
    let text = line_text(&decoded[..length], max_bytes).ok_or(InvalidText)?;
    Ok((!text.is_empty()).then_some(text))
}

#[cfg(test)]
mod tests {
    use super::{
        BlockedKind, MAX_MESSAGE_BYTES, MAX_PAYLOAD_BYTES, MAX_TITLE_BYTES, ProgramStatusReport,
        QUERY_REPLY, ReportedState, is_query, parse,
    };

    /// The base64 of `count` bytes `A`.
    fn encoded_text(count: usize) -> String {
        assert_eq!(count % 3, 0);
        "QUFB".repeat(count / 3)
    }

    fn report(state: ReportedState) -> ProgramStatusReport {
        ProgramStatusReport {
            id: String::new(),
            state,
            kind: None,
            progress: None,
            app: None,
            title: None,
            message: None,
        }
    }

    #[test]
    fn a_report_with_each_key_gives_each_field() {
        let payload = b"state=blocked:kind=permission:app=terraform:id=deploy/eu-west:progress=40:\
                        title=RVUgV2VzdA==:msg=QXBwbHkgMyB0byBhZGQsIDEgdG8gY2hhbmdlLCAwIHRvIGRlc3Ryb3k/";

        assert_eq!(
            parse(payload),
            Some(ProgramStatusReport {
                id: "deploy/eu-west".to_owned(),
                state: ReportedState::Blocked,
                kind: Some(BlockedKind::Permission),
                progress: Some(40),
                app: Some("terraform".to_owned()),
                title: Some("EU West".to_owned()),
                message: Some("Apply 3 to add, 1 to change, 0 to destroy?".to_owned()),
            })
        );
    }

    #[test]
    fn a_clear_report_keeps_the_path_of_the_record_to_remove() {
        let cleared = parse(b"state=clear:id=build/test").unwrap();

        assert_eq!(cleared.state, ReportedState::Clear);
        assert_eq!(cleared.id, "build/test");
        assert_eq!(parse(b"state=clear"), Some(report(ReportedState::Clear)));
    }

    #[test]
    fn pairs_that_are_not_valid_or_not_known_are_ignored_and_the_last_value_is_used() {
        let payload = b"novalue:=empty-key:state=idle:app=b@d:colour=red: state = working :app=first:app=last";

        let parsed = parse(payload).unwrap();

        assert_eq!(parsed.state, ReportedState::Working);
        assert_eq!(parsed.app.as_deref(), Some("last"));
    }

    #[test]
    fn kind_and_progress_apply_only_to_the_states_that_have_them() {
        let idle = parse(b"state=idle:kind=question:progress=10").unwrap();
        assert_eq!((idle.kind, idle.progress), (None, None));

        let working = parse(b"state=working:kind=question:progress=100").unwrap();
        assert_eq!((working.kind, working.progress), (None, Some(100)));

        let blocked = parse(b"state=blocked:kind=later:progress=0").unwrap();
        assert_eq!((blocked.kind, blocked.progress), (None, Some(0)));

        for progress in ["101", "-1", "+5", "1.5", "", "999999"] {
            let payload = format!("state=working:progress={progress}");
            assert_eq!(
                parse(payload.as_bytes()).unwrap().progress,
                None,
                "{progress}"
            );
        }
    }

    #[test]
    fn an_app_that_is_not_a_name_is_absent_and_the_report_stays() {
        assert_eq!(
            parse(b"state=idle:app=a/b"),
            Some(report(ReportedState::Idle))
        );
        assert_eq!(parse(b"state=idle:app="), Some(report(ReportedState::Idle)));
    }

    #[test]
    fn base64_without_padding_is_read() {
        let parsed = parse(b"state=done:msg=RG9jcyBidWlsdA").unwrap();

        assert_eq!(parsed.message.as_deref(), Some("Docs built"));
    }

    #[test]
    fn a_report_without_a_known_state_is_discarded() {
        assert_eq!(parse(b"app=cargo"), None);
        assert_eq!(parse(b"state=paused"), None);
        assert_eq!(parse(b"state=idle:state=later"), None);
        assert_eq!(parse(b""), None);
    }

    #[test]
    fn an_id_that_is_not_a_path_of_names_discards_the_report() {
        let long_segment = "a".repeat(33);
        for id in [
            "",
            "/",
            "/a",
            "a//b",
            "a/",
            "a b",
            "a,b",
            "a\u{e9}",
            "a/b/c/d/e/f/g/h/i",
            long_segment.as_str(),
        ] {
            let payload = format!("state=clear:id={id}");
            assert_eq!(parse(payload.as_bytes()), None, "{id:?}");
        }
        let deepest = parse(b"state=idle:id=a/b/c/d/e/f/g/h").unwrap();
        assert_eq!(deepest.id, "a/b/c/d/e/f/g/h");
    }

    #[test]
    fn a_value_or_a_key_above_its_limit_discards_the_report() {
        let cases = [
            format!("state=idle:{}=1", "k".repeat(17)),
            format!("state=idle:app={}", "a".repeat(33)),
            format!("state=idle:app={}:app=short", "a".repeat(33)),
            format!("state=idle:id={}", vec!["a".repeat(31); 5].join("/")),
            format!("state=idle:title={}", encoded_text(MAX_TITLE_BYTES + 3)),
            format!("state=idle:msg={}", encoded_text(MAX_MESSAGE_BYTES + 1)),
            format!("state=idle:msg={}", "QUFB".repeat(684)),
        ];
        for payload in cases {
            assert_eq!(parse(payload.as_bytes()), None, "{payload}");
        }

        let at_the_limits = format!(
            "state=idle:{}=1:app={}:title={}:msg={}",
            "k".repeat(16),
            "a".repeat(32),
            encoded_text(MAX_TITLE_BYTES),
            encoded_text(2046),
        );
        let parsed = parse(at_the_limits.as_bytes()).unwrap();
        assert_eq!(parsed.title.unwrap().len(), MAX_TITLE_BYTES);
        assert_eq!(parsed.message.unwrap().len(), 2046);
    }

    #[test]
    fn a_payload_longer_than_the_sequence_limit_is_discarded() {
        let fill = |length: usize| format!("state=idle:x={}", "a".repeat(length - 13));

        assert!(parse(fill(MAX_PAYLOAD_BYTES).as_bytes()).is_some());
        assert_eq!(parse(fill(MAX_PAYLOAD_BYTES + 1).as_bytes()), None);
    }

    #[test]
    fn text_that_is_not_one_line_of_utf8_in_base64_discards_the_report() {
        let cases = [
            ("not base64", "state=idle:msg=QUJ"),
            ("a byte that base64 does not have", "state=idle:title=QU.B"),
            ("a line feed: 'a\\nb'", "state=idle:msg=YQpi"),
            ("an escape character", "state=idle:title=G1sxbQ=="),
            ("the C1 control U+009B", "state=idle:msg=wps="),
            ("not UTF-8: 0xFF 0xFE", "state=idle:msg=//4="),
        ];
        for (what, payload) in cases {
            assert_eq!(parse(payload.as_bytes()), None, "{what}");
        }
    }

    #[test]
    fn the_query_reply_lists_the_states_and_the_kinds_that_the_parser_reads() {
        let reply = std::str::from_utf8(QUERY_REPLY).unwrap();
        let body = reply
            .strip_prefix("\x1b]7501;?:")
            .and_then(|rest| rest.strip_suffix("\x1b\\"))
            .unwrap();
        let (states, kinds) = body.split_once(':').unwrap();

        let states: Vec<&str> = states.strip_prefix("states=").unwrap().split(',').collect();
        assert_eq!(states, ["idle", "working", "done", "blocked", "error"]);
        for state in states {
            assert!(
                parse(format!("state={state}").as_bytes()).is_some(),
                "{state}"
            );
        }
        let kinds: Vec<&str> = kinds.strip_prefix("kinds=").unwrap().split(',').collect();
        assert_eq!(kinds, ["permission", "question", "auth"]);
        for kind in kinds {
            let blocked = parse(format!("state=blocked:kind={kind}").as_bytes()).unwrap();
            assert!(blocked.kind.is_some(), "{kind}");
        }
    }

    #[test]
    fn only_a_single_question_mark_is_the_query() {
        assert!(is_query(b"?"));
        assert!(!is_query(b"??"));
        assert!(!is_query(b"state=idle"));
        assert_eq!(parse(b"?"), None);
    }
}
