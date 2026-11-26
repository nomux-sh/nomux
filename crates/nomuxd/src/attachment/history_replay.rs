//! Sends output from the journal of a session to a client: the bytes that
//! the ring buffer of the holder has no more, and the end of an earlier
//! epoch. `spec/frame.md` of the `proto` repository has the rules.

use std::io;

use nomux_holder_protocol::journal::MAX_RANGE_LENGTH;
use nomux_holder_protocol::ring::RingReader;
use tokio::task::yield_now;

use super::frame_sink::FrameSink;
use crate::session_journal::SessionJournal;

/// What a client asks of an epoch that is not the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EarlierOutput {
    /// The client has the output before this position.
    From(u64),
    /// The client has no output: it gets the last screen that the journal
    /// has, and the output after it.
    FromLastCheckpoint,
}

/// Sends the end of the earlier epoch `epoch` and then the mark that the
/// command started again. The output ends at the first byte that the
/// journal does not have.
pub(super) async fn earlier_epoch(
    sink: &mut FrameSink,
    journal: &SessionJournal,
    epoch: u32,
    output: EarlierOutput,
) -> io::Result<()> {
    let start = match output {
        EarlierOutput::From(position) => Some(position),
        EarlierOutput::FromLastCheckpoint => match journal.last_checkpoint(epoch)? {
            Some(checkpoint) => {
                sink.snapshot(epoch, checkpoint.position, &checkpoint.redraw_sequence)
                    .await?;
                Some(checkpoint.position)
            }
            None => None,
        },
    };
    if let Some(mut next) = start {
        while let Some(range) = journal.read(epoch, next, MAX_RANGE_LENGTH).await? {
            if range.position != next {
                sink.history_gap(epoch, next).await?;
                break;
            }
            sink.output(epoch, next, &range.bytes).await?;
            next += range.bytes.len() as u64;
            yield_now().await;
        }
    }
    sink.restored().await
}

/// Sends the output of the current epoch from the position `from`, which
/// the ring buffer has no more, until the ring buffer has the next byte.
/// The first bytes of the ring buffer are in `buffer` then: the result is
/// their number.
///
/// Returns `None` if the journal cannot give the client a state from which
/// the output continues: the caller then sends the current screen. With
/// `tell_gaps`, the client learns that output is gone, and it gets the
/// oldest screen and the output that the journal has after a gap.
pub(super) async fn catch_up(
    sink: &mut FrameSink,
    journal: &SessionJournal,
    reader: &mut RingReader,
    buffer: &mut [u8],
    epoch: u32,
    from: u64,
    tell_gaps: bool,
) -> io::Result<Option<usize>> {
    let mut next = from;
    loop {
        let range = journal.read(epoch, next, MAX_RANGE_LENGTH).await?;
        let journal_has_more = range.is_some();
        match range {
            Some(range) if range.position == next => {
                sink.output(epoch, next, &range.bytes).await?;
                next += range.bytes.len() as u64;
            }
            // The journal continues after a gap.
            Some(range) if tell_gaps => {
                sink.history_gap(epoch, next).await?;
                match oldest_screen(sink, journal, epoch, range.position).await? {
                    Some(position) => next = position,
                    None => return Ok(None),
                }
                continue;
            }
            Some(_) => return Ok(None),
            None => {}
        }
        // One task serves all the clients.
        yield_now().await;
        reader.seek(next);
        if let Ok(count) = reader.read(buffer) {
            return Ok(Some(count));
        }
        if !journal_has_more {
            // The journal stopped, and the ring buffer is after its end.
            if tell_gaps {
                sink.history_gap(epoch, next).await?;
            }
            return Ok(None);
        }
    }
}

/// Sends the oldest screen that the journal has at the position `from` or
/// after it. Returns the position of the screen, from which the output
/// continues, or `None` if the journal has no such screen.
async fn oldest_screen(
    sink: &mut FrameSink,
    journal: &SessionJournal,
    epoch: u32,
    from: u64,
) -> io::Result<Option<u64>> {
    let Some(checkpoint) = journal.oldest_checkpoint(epoch, from)? else {
        return Ok(None);
    };
    sink.snapshot(epoch, checkpoint.position, &checkpoint.redraw_sequence)
        .await?;
    Ok(Some(checkpoint.position))
}
