//! The difference between two versions of a settings file, as a unified
//! diff.

use std::path::Path;

use similar::TextDiff;

/// The lines around a change that the diff shows.
const CONTEXT_LINES: usize = 3;

/// The unified diff from `before` to `after` of the file at `path`. A file
/// that does not exist is `/dev/null`. Empty if nothing changes.
pub(super) fn unified_diff(path: &Path, before: Option<&[u8]>, after: Option<&[u8]>) -> String {
    if before == after {
        return String::new();
    }
    let name = path.display().to_string();
    let header = |content: Option<&[u8]>| match content {
        Some(_) => name.clone(),
        None => "/dev/null".to_owned(),
    };
    let old = String::from_utf8_lossy(before.unwrap_or_default());
    let new = String::from_utf8_lossy(after.unwrap_or_default());
    TextDiff::from_lines(old.as_ref(), new.as_ref())
        .unified_diff()
        .context_radius(CONTEXT_LINES)
        .header(&header(before), &header(after))
        .to_string()
}
