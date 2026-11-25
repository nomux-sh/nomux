//! The file name of a segment. The names are the index of the journal: a
//! reader finds the segment of a position without a look into the files.

use std::ffi::OsStr;

const EPOCH_DIGITS: usize = 8;
const POSITION_DIGITS: usize = 16;
const EXTENSION: &str = ".journal";

/// What the file name of a segment says. The order of the values is the
/// order of the output: first the epoch, then the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SegmentName {
    /// The epoch of the holder that wrote the segment.
    pub epoch: u32,
    /// The position of the first output byte of the segment.
    pub start: u64,
}

impl SegmentName {
    /// The file name: the epoch (8 hexadecimal digits), `-`, the start
    /// position (16 hexadecimal digits) and `.journal`. Thus the order of
    /// the names as text is the order of the output.
    #[must_use]
    pub fn file_name(self) -> String {
        format!("{:08x}-{:016x}{EXTENSION}", self.epoch, self.start)
    }

    /// Reads a file name. Returns `None` for a file that is not a segment.
    #[must_use]
    pub fn parse(file_name: &OsStr) -> Option<Self> {
        let digits = file_name.to_str()?.strip_suffix(EXTENSION)?;
        let (epoch, start) = digits.split_once('-')?;
        if epoch.len() != EPOCH_DIGITS || start.len() != POSITION_DIGITS {
            return None;
        }
        let is_lower_hex = |text: &str| {
            text.bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        };
        if !is_lower_hex(epoch) || !is_lower_hex(start) {
            return None;
        }
        Some(Self {
            epoch: u32::from_str_radix(epoch, 16).ok()?,
            start: u64::from_str_radix(start, 16).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_of_the_file_names_is_the_order_of_the_output() {
        let names = [
            SegmentName { epoch: 1, start: 0 },
            SegmentName { epoch: 1, start: 9 },
            SegmentName {
                epoch: 1,
                start: 0x10,
            },
            SegmentName {
                epoch: 1,
                start: u64::MAX,
            },
            SegmentName { epoch: 2, start: 0 },
            SegmentName {
                epoch: 0x10,
                start: 0,
            },
        ];
        let file_names: Vec<String> = names.iter().map(|name| name.file_name()).collect();
        assert!(file_names.is_sorted(), "{file_names:?}");
        for (name, file_name) in names.iter().zip(&file_names) {
            assert_eq!(SegmentName::parse(OsStr::new(file_name)), Some(*name));
        }
    }

    #[test]
    fn a_name_that_the_writer_does_not_make_is_not_a_segment() {
        for file_name in [
            "session",
            "00000001-0000000000000000",
            "1-0.journal",
            "0000000G-0000000000000000.journal",
            "+0000001-0000000000000000.journal",
            "00000001-000000000000000A.journal",
            "00000001-0000000000000000.journal.tmp",
        ] {
            assert_eq!(
                SegmentName::parse(OsStr::new(file_name)),
                None,
                "{file_name}"
            );
        }
    }
}
