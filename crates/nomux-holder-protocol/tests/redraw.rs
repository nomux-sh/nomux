//! The shared memory of a redraw sequence, with the writer and the reader
//! on the same shared file.

use std::io;

use nomux_holder_protocol::redraw;

fn shared_memory(length: usize) -> std::fs::File {
    let file = tempfile::tempfile().expect("the test helper works");
    file.set_len(length as u64).expect("the test helper works");
    file
}

#[test]
fn the_reader_gets_the_sequence_that_the_writer_wrote() {
    // Longer than one page and not a multiple of the page size.
    let sequence: Vec<u8> = (0..70_001u32).map(|index| index.to_le_bytes()[0]).collect();
    let memory = shared_memory(sequence.len());

    redraw::write(&memory, &sequence).unwrap();

    let length = u32::try_from(sequence.len()).unwrap();
    assert_eq!(redraw::read(&memory, length).unwrap(), sequence);
}

#[test]
fn a_length_that_the_memory_does_not_have_is_rejected() {
    let memory = shared_memory(100);
    let error = redraw::read(&memory, 101).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_length_above_the_limit_is_rejected_before_the_memory_is_mapped() {
    let over_the_limit = redraw::MAX_LENGTH + 1;
    let memory = shared_memory(over_the_limit);
    let error = redraw::read(&memory, u32::try_from(over_the_limit).unwrap()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
