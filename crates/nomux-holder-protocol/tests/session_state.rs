//! The text of the session metadata file.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use nomux_holder_protocol::session_state::{InvalidMetadata, RestorePolicy, SessionMetadata};
use nomux_holder_protocol::wire::{ChildStatus, WindowSize};

fn metadata() -> SessionMetadata {
    SessionMetadata {
        epoch: 2,
        started: 1_795_651_200,
        size: WindowSize {
            rows: 24,
            columns: 80,
        },
        restore_policy: RestorePolicy::OnFocus,
        profile: String::new(),
        working_directory: PathBuf::from("/home/user/my project"),
        command: OsString::from("/bin/sh"),
        arguments: vec![OsString::from("-c"), OsString::from("echo hello")],
        status: ChildStatus::Running,
        ended: None,
    }
}

const TEXT: &str = "\
nomux-session 1
epoch 2
started 1795651200
rows 24
columns 80
restore on-focus
profile
directory /home/user/my project
command /bin/sh
argument -c
argument echo hello
status running
";

#[test]
fn the_text_of_the_file_does_not_change() {
    assert_eq!(metadata().encode(), TEXT);
    assert_eq!(SessionMetadata::parse(TEXT), Ok(metadata()));

    let ended = SessionMetadata {
        status: ChildStatus::Signaled { signal: 15 },
        ended: Some(1_795_651_300),
        ..metadata()
    };
    let text = ended.encode();
    assert!(
        text.ends_with("status signaled 15\nended 1795651300\n"),
        "{text}"
    );
    assert_eq!(SessionMetadata::parse(&text), Ok(ended));
}

#[test]
fn a_command_with_each_byte_is_one_line_for_each_argument_and_comes_back() {
    let unusual = SessionMetadata {
        working_directory: PathBuf::from(OsString::from_vec(b"/tmp/\xff\n dir ".to_vec())),
        command: OsString::from("my program"),
        arguments: vec![
            OsString::from(""),
            OsString::from("two\nlines"),
            OsString::from(r"back\slash \n"),
            OsString::from(" spaces "),
            OsString::from_vec(vec![0, 0x1b, 0x7f, 0x80, 0xfe]),
            OsString::from("status exited 0"),
        ],
        status: ChildStatus::Exited { code: -1 },
        ended: Some(5),
        ..metadata()
    };

    let text = unusual.encode();

    assert_eq!(text.lines().count(), TEXT.lines().count() + 5);
    assert_eq!(SessionMetadata::parse(&text), Ok(unusual));
}

#[test]
fn a_key_of_a_later_version_is_ignored_and_a_person_can_leave_out_the_optional_keys() {
    let text = "\
nomux-session 1
later-key some value
epoch 4
started 10
rows 30
columns 90
directory /
command top
";
    let parsed = SessionMetadata::parse(text).unwrap();

    assert_eq!(parsed.epoch, 4);
    assert_eq!(parsed.restore_policy, RestorePolicy::OnFocus);
    assert_eq!((parsed.status, parsed.ended), (ChildStatus::Running, None));
    assert_eq!(parsed.arguments, [""; 0]);
}

#[test]
fn a_text_that_is_not_the_metadata_of_this_version_is_refused() {
    let with_line = |old: &str, new: &str| TEXT.replace(old, new);

    assert_eq!(SessionMetadata::parse(""), Err(InvalidMetadata::Line(1)));
    assert_eq!(
        SessionMetadata::parse(&with_line("nomux-session 1", "nomux-session 2")),
        Err(InvalidMetadata::Line(1))
    );
    assert_eq!(
        SessionMetadata::parse(&with_line("epoch 2\n", "")),
        Err(InvalidMetadata::Missing("epoch"))
    );
    for (old, new, line) in [
        ("epoch 2", "epoch 0", 2),
        ("rows 24", "rows many", 4),
        ("restore on-focus", "restore sometimes", 6),
        ("command /bin/sh", r"command /bin/\sh", 9),
        ("status running", "status exited", 12),
    ] {
        assert_eq!(
            SessionMetadata::parse(&with_line(old, new)),
            Err(InvalidMetadata::Line(line)),
            "{new}"
        );
    }
}
