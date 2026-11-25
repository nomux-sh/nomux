//! The encoding of the program reports and their shared memory.

use std::io;

use nomux_holder_protocol::program_reports::{
    self, BlockedKind, InputLine, Notification, ProgramReports, ProgramState, ProgramStatus,
    PromptMark,
};
use nomux_holder_protocol::wire::DecodeError;

fn root_status() -> ProgramStatus {
    ProgramStatus {
        id: String::new(),
        state: ProgramState::Working,
        kind: None,
        progress: None,
        app: Some("sh".to_owned()),
        title: None,
        message: None,
        revision: 2,
    }
}

fn reports() -> ProgramReports {
    ProgramReports {
        revision: 9,
        input_line: InputLine {
            mark: Some(PromptMark::CommandEnd),
            exit_code: Some(-3),
            input_after_mark: true,
            revision: 7,
        },
        statuses: vec![
            root_status(),
            ProgramStatus {
                id: "deploy/eu-west".to_owned(),
                state: ProgramState::Blocked,
                kind: Some(BlockedKind::Auth),
                progress: Some(100),
                app: None,
                title: Some("EU West".to_owned()),
                message: Some("Approve the deploy to eu-west ✓".to_owned()),
                revision: 8,
            },
        ],
        notifications: vec![
            Notification {
                title: None,
                body: "Build finished".to_owned(),
                revision: 4,
            },
            Notification {
                title: Some("Deploy".to_owned()),
                body: String::new(),
                revision: 9,
            },
        ],
    }
}

fn encoded(reports: &ProgramReports) -> Vec<u8> {
    let mut bytes = Vec::new();
    reports.encode(&mut bytes);
    bytes
}

/// Adds bytes to the end of the header and to the end of each block, as a
/// later minor version of the protocol can do.
fn with_added_fields(bytes: &[u8]) -> Vec<u8> {
    let (header_length, rest) = bytes
        .split_first_chunk::<4>()
        .expect("the test helper works");
    let header_length = u32::from_le_bytes(*header_length);
    let (header, mut blocks) = rest.split_at(header_length as usize);
    let mut output = (header_length + 3).to_le_bytes().to_vec();
    output.extend_from_slice(header);
    output.extend_from_slice(&[0xAA; 3]);
    while let Some((length_field, rest)) = blocks.split_first_chunk::<2>() {
        let length = u16::from_le_bytes(*length_field);
        let (block, rest) = rest.split_at(usize::from(length));
        output.extend_from_slice(&(length + 5).to_le_bytes());
        output.extend_from_slice(block);
        output.extend_from_slice(&[0xBB; 5]);
        blocks = rest;
    }
    output
}

#[test]
fn the_bytes_of_the_program_reports_do_not_change() {
    let reports = ProgramReports {
        revision: 9,
        input_line: InputLine {
            mark: Some(PromptMark::InputStart),
            exit_code: Some(-3),
            input_after_mark: true,
            revision: 7,
        },
        statuses: vec![ProgramStatus {
            kind: Some(BlockedKind::Question),
            progress: Some(40),
            id: "a/b".to_owned(),
            ..root_status()
        }],
        notifications: vec![Notification {
            title: Some("T".to_owned()),
            body: "ok".to_owned(),
            revision: 4,
        }],
    };

    assert_eq!(
        encoded(&reports),
        [
            28, 0, 0, 0, // header length after this field
            9, 0, 0, 0, 0, 0, 0, 0, // revision
            2, // last prompt mark: input start
            3, // flags: input after the mark, the exit code is present
            0, 0, // zero
            0xFD, 0xFF, 0xFF, 0xFF, // exit code -3
            7, 0, 0, 0, 0, 0, 0, 0, // revision of the input line state
            1, 0, // number of program status records
            1, 0, // number of notifications
            25, 0, // length of the record
            2, 0, 0, 0, 0, 0, 0, 0,  // revision of the record
            2,  // state: working
            2,  // kind: question
            40, // progress
            0,  // zero
            3, 0, b'a', b'/', b'b', // id
            2, 0, b's', b'h', // app
            0, 0, // title
            0, 0, // message
            15, 0, // length of the notification
            4, 0, 0, 0, 0, 0, 0, 0, // revision of the notification
            1, 0, b'T', // title
            2, 0, b'o', b'k', // body
        ]
    );
}

#[test]
fn the_program_reports_decode_to_the_value_that_was_encoded() {
    assert_eq!(ProgramReports::decode(&encoded(&reports())), Ok(reports()));

    let empty = ProgramReports::default();
    assert_eq!(ProgramReports::decode(&encoded(&empty)), Ok(empty));
}

#[test]
fn fields_that_a_later_version_adds_to_the_header_or_to_a_block_are_ignored() {
    let later = with_added_fields(&encoded(&reports()));

    assert_eq!(ProgramReports::decode(&later), Ok(reports()));
}

#[test]
fn values_of_a_later_version_do_not_make_the_other_records_unreadable() {
    const MARK: usize = 12;
    const FIRST_RECORD: usize = 32;
    const STATE_IN_RECORD: usize = 2 + 8;
    const KIND_IN_RECORD: usize = STATE_IN_RECORD + 1;
    let mut bytes = encoded(&reports());
    let first_record_length = usize::from(bytes[FIRST_RECORD]);
    let second_record = FIRST_RECORD + 2 + first_record_length;
    assert_eq!(bytes[second_record + KIND_IN_RECORD], 3, "the kind auth");
    bytes[MARK] = 9;
    bytes[FIRST_RECORD + STATE_IN_RECORD] = 9;
    bytes[second_record + KIND_IN_RECORD] = 9;

    let decoded = ProgramReports::decode(&bytes).unwrap();

    assert_eq!(decoded.input_line.mark, None);
    assert_eq!(decoded.statuses.len(), 1);
    assert_eq!(decoded.statuses[0].id, "deploy/eu-west");
    assert_eq!(decoded.statuses[0].kind, None);
    assert_eq!(decoded.notifications, reports().notifications);
}

#[test]
fn bytes_that_end_before_the_last_field_are_invalid() {
    let bytes = encoded(&reports());

    for length in 0..bytes.len() {
        assert_eq!(
            ProgramReports::decode(&bytes[..length]),
            Err(DecodeError::InvalidPayload),
            "{length} of {} bytes",
            bytes.len()
        );
    }
}

#[test]
fn a_text_that_is_not_utf8_is_invalid() {
    let mut bytes = encoded(&reports());
    let title = bytes
        .windows(6)
        .position(|window| window == b"Deploy")
        .unwrap();
    bytes[title] = 0xFF;

    assert_eq!(
        ProgramReports::decode(&bytes),
        Err(DecodeError::InvalidPayload)
    );
}

#[test]
fn the_reader_gets_the_program_reports_that_the_writer_wrote() {
    let bytes = encoded(&reports());
    let memory = tempfile::tempfile().unwrap();
    memory.set_len(bytes.len() as u64).unwrap();

    program_reports::write(&memory, &bytes).unwrap();

    let length = u32::try_from(bytes.len()).unwrap();
    assert_eq!(program_reports::read(&memory, length).unwrap(), reports());
    let error = program_reports::read(&memory, length + 1).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_length_above_the_limit_is_rejected_before_the_memory_is_mapped() {
    let over_the_limit = program_reports::MAX_LENGTH + 1;
    let memory = tempfile::tempfile().unwrap();
    memory.set_len(over_the_limit as u64).unwrap();

    let error = program_reports::read(&memory, u32::try_from(over_the_limit).unwrap()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
