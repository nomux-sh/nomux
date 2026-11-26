//! The scrollback of a session: the output that its command wrote, read in
//! parts from the newest bytes to the oldest.

use crate::{Client, ClientError, OutputPosition};

/// The first epoch of a session.
const FIRST_EPOCH: u32 = 1;

/// Output bytes of one epoch of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedOutput {
    /// The epoch of the bytes.
    pub epoch: u32,
    /// The sequence number of the first byte. With no bytes, the position
    /// before which the daemon has no output of the epoch.
    pub seq: u64,
    /// The output bytes, as the command wrote them. They can start or end
    /// in the middle of an escape sequence or of a UTF-8 character.
    pub bytes: Vec<u8>,
}

/// Which output of a session a client asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureRange {
    /// The epoch of the output. `None` is the newest epoch of the session.
    pub epoch: Option<u32>,
    /// The sequence number after the last byte that the client asks for.
    /// `None` is the end of the output of the epoch.
    pub before: Option<u64>,
    /// The largest number of bytes. `None` is the limit of the daemon:
    /// 262144 bytes.
    pub max_bytes: Option<u32>,
}

impl Client {
    /// Reads the last `lines` lines that the command of the session `id`
    /// wrote, also the lines that are not on its screen now and the lines
    /// from before a restore. A line ends with a line feed. The bytes are
    /// the output as the command wrote it, with its escape sequences.
    ///
    /// The result has fewer lines if the daemon does not have more output.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier.
    pub async fn capture_lines(&mut self, id: &str, lines: usize) -> Result<Vec<u8>, ClientError> {
        Ok(self.capture_tail(id, lines).await?.0)
    }

    /// The last `lines` lines of the session `id`, as
    /// [`capture_lines`](Self::capture_lines) gives them, and the position
    /// after them: an attach that resumes at this position gives each byte
    /// that the command writes after the lines.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier.
    pub async fn capture_tail(
        &mut self,
        id: &str,
        lines: usize,
    ) -> Result<(Vec<u8>, OutputPosition), ClientError> {
        let mut end: Option<OutputPosition> = None;
        // The parts, from the newest one to the oldest one.
        let mut parts: Vec<Vec<u8>> = Vec::new();
        let mut line_feeds = 0;
        let mut range = CaptureRange::default();
        // One line feed more than `lines` is before the first line.
        while line_feeds <= lines {
            let part = self.capture(id, range).await?;
            end.get_or_insert(OutputPosition {
                epoch: part.epoch,
                seq: part.seq + part.bytes.len() as u64,
            });
            line_feeds += part.bytes.split(|byte| *byte == b'\n').count() - 1;
            let epoch_is_complete = part.bytes.is_empty() || part.seq == 0;
            if !part.bytes.is_empty() {
                parts.push(part.bytes);
            }
            range = match (epoch_is_complete, part.epoch) {
                (false, epoch) => CaptureRange {
                    epoch: Some(epoch),
                    before: Some(part.seq),
                    max_bytes: None,
                },
                (true, ..=FIRST_EPOCH) => break,
                // The output from before a restore is in the epoch before.
                (true, epoch) => CaptureRange {
                    epoch: Some(epoch - 1),
                    ..CaptureRange::default()
                },
            };
        }
        let output: Vec<u8> = parts.into_iter().rev().flatten().collect();
        // The loop reads one part at least.
        let end = end.unwrap_or(OutputPosition { epoch: 0, seq: 0 });
        Ok((last_lines(&output, lines).to_vec(), end))
    }
}

/// The last `lines` lines of `output`. A line feed at the end of the output
/// ends the last line: it does not start an empty line.
fn last_lines(output: &[u8], lines: usize) -> &[u8] {
    if lines == 0 {
        return &[];
    }
    let content = output.strip_suffix(b"\n").unwrap_or(output);
    let line_feeds = content
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, byte)| **byte == b'\n');
    match line_feeds.map(|(index, _)| index).nth(lines - 1) {
        Some(line_feed) => &output[line_feed + 1..],
        None => output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_lines_start_after_a_line_feed() {
        assert_eq!(last_lines(b"a\nb\nc\n", 2), b"b\nc\n");
        assert_eq!(last_lines(b"a\nb\nc", 2), b"b\nc");
        assert_eq!(last_lines(b"a\nb\nc\n", 1), b"c\n");
        // An empty line is a line.
        assert_eq!(last_lines(b"a\n\n\n", 2), b"\n\n");
        // Fewer lines than the caller asks for: all the output.
        assert_eq!(last_lines(b"a\nb", 5), b"a\nb");
        assert_eq!(last_lines(b"a\nb\n", 0), b"");
        assert_eq!(last_lines(b"", 3), b"");
    }
}
