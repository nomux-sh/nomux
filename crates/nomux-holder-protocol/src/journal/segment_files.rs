//! The segment files of a journal directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::segment_name::SegmentName;

/// One segment file of a journal directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentFile {
    /// What the name of the file says.
    pub name: SegmentName,
    /// The path of the file.
    pub path: PathBuf,
    /// The number of bytes of the file.
    pub length: u64,
}

/// Lists the segment files of `journal_directory` in the order of the
/// output: the oldest segment is first. A file with a different name is
/// ignored. A directory that does not exist has no segment.
///
/// # Errors
///
/// Returns an error if the directory cannot be read.
pub fn segment_files(journal_directory: &Path) -> io::Result<Vec<SegmentFile>> {
    let entries = match fs::read_dir(journal_directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(name) = SegmentName::parse(&entry.file_name()) else {
            continue;
        };
        // A file that the holder removed between the two calls is no
        // segment.
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_file() {
            files.push(SegmentFile {
                name,
                path: entry.path(),
                length: metadata.len(),
            });
        }
    }
    files.sort_unstable_by_key(|file| file.name);
    Ok(files)
}
