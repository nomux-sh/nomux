//! The identity of a process: the content of the `pid` file.

use std::fmt;
use std::io;
use std::str::FromStr;

use super::process_record;

/// A process ID together with the start time of the process.
///
/// The operating system gives a process ID to a new process after the old
/// process ended. The start time is different then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// The process ID.
    pub pid: u32,
    /// The start time of the process, in the unit of the operating system:
    /// clock ticks after the system start on Linux, microseconds after the
    /// Unix epoch on macOS. Compare it only for equality.
    pub start_time: u64,
}

impl ProcessIdentity {
    /// The identity of this process.
    ///
    /// # Errors
    ///
    /// Returns an error if the operating system does not give the start
    /// time.
    pub fn current() -> io::Result<Self> {
        let pid = std::process::id();
        Self::of(pid)?.ok_or_else(|| io::Error::other("the current process has no start time"))
    }

    /// The identity of the process that has `pid` now. Returns `None` if no
    /// such process runs.
    ///
    /// # Errors
    ///
    /// Returns an error if the operating system does not give the start
    /// time of a process that exists.
    pub fn of(pid: u32) -> io::Result<Option<Self>> {
        let record = process_record::read(pid)?;
        Ok(record.map(|record| Self {
            pid,
            start_time: record.start_time,
        }))
    }
}

impl fmt::Display for ProcessIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.pid, self.start_time)
    }
}

impl FromStr for ProcessIdentity {
    type Err = io::Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid pid file");
        let mut fields = text.split_ascii_whitespace();
        let pid = fields.next().and_then(|field| field.parse().ok());
        let start_time = fields.next().and_then(|field| field.parse().ok());
        match (pid, start_time) {
            (Some(pid), Some(start_time)) => Ok(Self { pid, start_time }),
            _ => Err(invalid()),
        }
    }
}
