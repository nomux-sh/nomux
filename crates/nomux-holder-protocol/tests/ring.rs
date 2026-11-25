//! The ring buffer with a writer and a reader on the same shared file.

use std::convert::Infallible;
use std::io;
use std::os::unix::fs::FileExt;

use nomux_holder_protocol::ring::{Overrun, RingReader, RingWriter, region_length};

const CAPACITY: usize = 4096;

fn shared_memory() -> std::fs::File {
    let file = tempfile::tempfile().expect("the test helper works");
    file.set_len(region_length(CAPACITY) as u64)
        .expect("the test helper works");
    file
}

fn ring() -> (RingWriter, RingReader) {
    let memory = shared_memory();
    let writer = RingWriter::create(&memory, CAPACITY).expect("the test helper works");
    let reader = RingReader::map(&memory).expect("the test helper works");
    (writer, reader)
}

fn pattern(start: usize, length: usize) -> Vec<u8> {
    (start..start + length)
        .map(|index| index.to_le_bytes()[0])
        .collect()
}

fn write_all(writer: &mut RingWriter, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        let written = writer
            .write_with(CAPACITY, |chunk| {
                let count = chunk.len().min(bytes.len());
                chunk[..count].copy_from_slice(&bytes[..count]);
                Ok::<_, Infallible>(count)
            })
            .expect("the test helper works");
        bytes = &bytes[written..];
    }
}

fn read_all(reader: &mut RingReader) -> Result<Vec<u8>, Overrun> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 1000];
    loop {
        match reader.read(&mut buffer)? {
            0 => return Ok(output),
            count => output.extend_from_slice(&buffer[..count]),
        }
    }
}

#[test]
fn output_that_wraps_around_the_end_of_the_data_is_read_in_sequence() {
    let (mut writer, mut reader) = ring();

    write_all(&mut writer, &pattern(0, 3000));
    assert_eq!(read_all(&mut reader).unwrap(), pattern(0, 3000));

    // 3000 + 3000 is larger than the capacity: the second write wraps.
    write_all(&mut writer, &pattern(3000, 3000));
    assert_eq!(read_all(&mut reader).unwrap(), pattern(3000, 3000));
    assert_eq!(reader.position(), 6000);
    assert_eq!(reader.write_position(), 6000);
}

#[test]
fn a_chunk_stops_at_the_end_of_the_data() {
    let (mut writer, _reader) = ring();
    write_all(&mut writer, &pattern(0, 3000));

    let chunk_length = writer
        .write_with(CAPACITY, |chunk| Ok::<_, Infallible>(chunk.len()))
        .unwrap();

    assert_eq!(chunk_length, CAPACITY - 3000);
}

#[test]
fn a_reader_that_is_more_than_the_capacity_behind_reports_the_lost_bytes() {
    let (mut writer, mut reader) = ring();
    write_all(&mut writer, &pattern(0, CAPACITY + 100));

    assert_eq!(
        read_all(&mut reader),
        Err(Overrun {
            lost: CAPACITY as u64 + 100
        })
    );

    // The reader continues at the write position.
    write_all(&mut writer, b"after the overrun");
    assert_eq!(read_all(&mut reader).unwrap(), b"after the overrun");
}

#[test]
fn a_read_during_a_claim_is_an_overrun() {
    let (mut writer, mut reader) = ring();
    write_all(&mut writer, &pattern(0, CAPACITY));

    // The reader is exactly one capacity behind. The next chunk of the
    // writer is the oldest part of the data, which the reader did not read.
    let mut result = Ok(0);
    writer
        .write_with(100, |_chunk| {
            let mut buffer = [0u8; 16];
            result = reader.read(&mut buffer);
            Ok::<_, Infallible>(0)
        })
        .unwrap();

    assert_eq!(
        result,
        Err(Overrun {
            lost: CAPACITY as u64
        })
    );
}

#[test]
fn a_new_reader_starts_at_the_oldest_byte_that_the_ring_buffer_has() {
    let memory = shared_memory();
    let mut writer = RingWriter::create(&memory, CAPACITY).unwrap();
    write_all(&mut writer, &pattern(0, 10_000));

    let mut reader = RingReader::map(&memory).unwrap();

    assert_eq!(reader.position(), 10_000 - CAPACITY as u64);
    assert_eq!(
        read_all(&mut reader).unwrap(),
        pattern(10_000 - CAPACITY, CAPACITY)
    );
}

#[test]
fn a_reader_continues_from_the_position_that_it_seeks() {
    let (mut writer, mut reader) = ring();
    write_all(&mut writer, &pattern(0, 3000));
    assert_eq!(read_all(&mut reader).unwrap(), pattern(0, 3000));

    reader.seek(1000);
    assert_eq!(read_all(&mut reader).unwrap(), pattern(1000, 2000));

    // A position that the writer did not reach is the write position.
    reader.seek(5000);
    assert_eq!(reader.position(), 3000);

    // The second write wraps: position 1000 is not in the ring buffer.
    write_all(&mut writer, &pattern(3000, 3000));
    reader.seek(1000);
    assert_eq!(read_all(&mut reader), Err(Overrun { lost: 5000 }));
}

/// A holder that the daemon does not trust can store each value in its
/// positions.
#[test]
fn a_read_over_the_largest_position_continues_at_position_0() {
    const COMMITTED_OFFSET: u64 = 40;
    let near_the_largest = u64::MAX - 1;
    let memory = shared_memory();
    let store_committed = |position: u64| {
        memory
            .write_all_at(&position.to_le_bytes(), COMMITTED_OFFSET)
            .expect("the test helper works");
    };
    let _writer = RingWriter::create(&memory, CAPACITY).expect("the test helper works");
    let mut reader = RingReader::map(&memory).expect("the test helper works");
    store_committed(near_the_largest);
    reader.seek(near_the_largest);

    store_committed(5);

    assert_eq!(reader.read(&mut [0; 100]), Ok(7));
    assert_eq!(reader.position(), 5);
}

#[test]
fn a_region_without_the_header_is_rejected() {
    let error = RingReader::map(shared_memory()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_capacity_that_is_not_a_power_of_two_is_rejected() {
    let error = RingWriter::create(shared_memory(), CAPACITY - 1).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}
