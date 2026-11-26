//! Examines the journal of a session without a read of its output.

use std::fs::File;
use std::io::{self, Read};
use std::ops::Range;
use std::path::Path;

use nomux_holder_protocol::journal::{SegmentFile, SegmentHeader, segment_files};

/// The fields of a segment header that say what the segment is: the magic
/// and the format version, the epoch, and the start position. A later
/// version of the format can change the other bytes.
const IDENTITY_FIELDS: [Range<usize>; 3] = [0..12, 16..20, 24..32];

/// Why a part of the journal in `journal_directory` cannot be read. Returns
/// the first problem, or `None` if each segment file starts with the header
/// of its name.
///
/// The check reads 32 bytes of each segment, thus it is fast for a full
/// journal. It does not find damage inside a segment: the reader of the
/// journal finds that when it reads the output.
pub(super) fn damage(journal_directory: &Path) -> Option<String> {
    let segments = match segment_files(journal_directory) {
        Ok(segments) => segments,
        Err(error) => return Some(format!("the journal cannot be read: {error}")),
    };
    segments.iter().find_map(|segment| {
        let problem = match has_header_of_its_name(segment) {
            Ok(true) => return None,
            Ok(false) => "does not start with the header of a segment".to_owned(),
            Err(error) => format!("cannot be read: {error}"),
        };
        Some(format!(
            "the journal segment {} {problem}",
            segment.name.file_name()
        ))
    })
}

fn has_header_of_its_name(segment: &SegmentFile) -> io::Result<bool> {
    let expected = SegmentHeader { name: segment.name }.encode();
    let mut header = [0; SegmentHeader::LENGTH];
    match File::open(&segment.path)?.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
        Err(error) => return Err(error),
    }
    Ok(IDENTITY_FIELDS
        .iter()
        .all(|field| header[field.clone()] == expected[field.clone()]))
}
