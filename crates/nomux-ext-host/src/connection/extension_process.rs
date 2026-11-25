//! The process of one extension and its three pipes.

use std::future;
use std::io;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use nomux_extension::frame::FrameError;
use nomux_extension::wire::v1::Envelope;
use prost::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::time::{Instant, timeout};

use super::envelope_reader::EnvelopeReader;
use super::flood_meter::FloodMeter;
use super::stderr_log::StderrLog;
use crate::config::Limits;
use crate::failure::Failure;

/// The room for one read of the standard error.
const STDERR_CHUNK: usize = 8 * 1024;

/// After the process ended, the host reads the rest of its standard error
/// for this time at most. A process that the extension started can keep the
/// pipe open.
const STDERR_DRAIN: Duration = Duration::from_millis(200);

/// What the process did.
pub(super) enum ProcessEvent {
    /// The extension sent one message.
    Envelope(Envelope),
    /// The process closed its standard output and ended.
    Exited(ExitStatus),
}

/// What one read of the pipes gave.
enum Read {
    Output(Result<Option<Envelope>, FrameError>),
    Stderr(io::Result<usize>),
}

pub(super) struct ExtensionProcess {
    child: Child,
    input: ChildStdin,
    output: EnvelopeReader<ChildStdout>,
    /// `None` after the process closed its standard error.
    stderr: Option<ChildStderr>,
    stderr_chunk: Vec<u8>,
    stderr_log: Arc<StderrLog>,
    stderr_flood: FloodMeter,
    write_limit: Duration,
    exit_limit: Duration,
}

impl ExtensionProcess {
    /// Takes the pipes of a process that `Launch::spawn` started.
    pub(super) fn new(
        mut child: Child,
        stderr_log: Arc<StderrLog>,
        limits: &Limits,
    ) -> Result<Self, Failure> {
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(input), Some(output), Some(stderr)) = pipes else {
            return Err(Failure::Spawn("the process has no pipes".to_owned()));
        };
        Ok(Self {
            child,
            input,
            output: EnvelopeReader::new(output),
            stderr: Some(stderr),
            stderr_chunk: Vec::with_capacity(STDERR_CHUNK),
            stderr_log,
            stderr_flood: FloodMeter::new(limits.stderr_flood_bytes, limits.stderr_flood_window),
            write_limit: limits.health,
            exit_limit: limits.shutdown,
        })
    }

    /// Waits for the subsequent message of the extension or for the end of
    /// the process. While it waits, it moves the standard error to the log.
    ///
    /// The end of the process is known from the end of its standard output,
    /// thus each message that the process sent comes before `Exited`.
    ///
    /// This function is cancel safe.
    pub(super) async fn next(&mut self) -> Result<ProcessEvent, Failure> {
        loop {
            let read = tokio::select! {
                output = self.output.next() => Read::Output(output),
                count = read_chunk(&mut self.stderr, &mut self.stderr_chunk) => Read::Stderr(count),
            };
            match read {
                Read::Output(Ok(Some(envelope))) => return Ok(ProcessEvent::Envelope(envelope)),
                Read::Output(Ok(None)) => {
                    let status = self.wait_for_exit().await;
                    return status
                        .map(ProcessEvent::Exited)
                        .ok_or(Failure::ClosedOutput);
                }
                Read::Output(Err(error)) => return Err(Failure::from_frame_error(error)),
                Read::Stderr(count) => self.record_stderr(&count)?,
            }
        }
    }

    /// Writes one message. The extension must take it in the permitted
    /// time: a process that does not read its input does not respond.
    pub(super) async fn send(&mut self, envelope: &Envelope) -> Result<(), Failure> {
        let frame = envelope.encode_length_delimited_to_vec();
        let write = async {
            self.input.write_all(&frame).await?;
            self.input.flush().await
        };
        match timeout(self.write_limit, write).await {
            Ok(Ok(())) => Ok(()),
            Err(_) => Err(Failure::InputBlocked),
            // A write fails when the process ended. Its exit status says
            // more than the error of the pipe.
            Ok(Err(error)) => Err(match self.wait_for_exit().await {
                Some(status) => Failure::Exited(status),
                None => Failure::Stream(error.to_string()),
            }),
        }
    }

    /// Ends the process, if it runs, and waits for its end.
    pub(super) async fn kill(&mut self) {
        // The only error is that the process already ended.
        let _ = self.child.kill().await;
    }

    /// Waits a short time for the end of a process that closed a pipe.
    /// Returns `None` if the process continues to run.
    async fn wait_for_exit(&mut self) -> Option<ExitStatus> {
        let status = timeout(self.exit_limit, self.child.wait())
            .await
            .ok()?
            .ok()?;
        // The last words of a process, for example a panic message.
        let rest_of_stderr = async {
            while self.stderr.is_some() {
                let count = read_chunk(&mut self.stderr, &mut self.stderr_chunk).await;
                if self.record_stderr(&count).is_err() {
                    break;
                }
            }
        };
        let _ = timeout(STDERR_DRAIN, rest_of_stderr).await;
        Some(status)
    }

    fn record_stderr(&mut self, count: &io::Result<usize>) -> Result<(), Failure> {
        if !matches!(count, Ok(1..)) {
            // The process closed the pipe, or the pipe failed. The process
            // can continue without it.
            self.stderr = None;
            return Ok(());
        }
        self.stderr_log.append(&self.stderr_chunk);
        if self
            .stderr_flood
            .is_flood(self.stderr_chunk.len(), Instant::now())
        {
            return Err(Failure::StderrFlood);
        }
        Ok(())
    }
}

/// Reads the subsequent bytes of the standard error into `chunk`. Does not
/// complete after the pipe closed.
async fn read_chunk(stderr: &mut Option<ChildStderr>, chunk: &mut Vec<u8>) -> io::Result<usize> {
    let Some(stderr) = stderr else {
        return future::pending().await;
    };
    chunk.clear();
    stderr.read_buf(chunk).await
}
