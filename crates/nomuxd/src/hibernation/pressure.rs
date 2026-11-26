//! The memory pressure of the host, as the operating system reports it.
//!
//! Linux: a trigger of the pressure stall information (PSI) on
//! `/proc/pressure/memory`. The kernel wakes the daemon when processes wait
//! for memory for [`PRESSURE`] stall time in its window, and at most one
//! time in each window. The daemon does not poll. A user without
//! privileges can make a trigger whose window is a multiple of 2 seconds.
//!
//! macOS: not implemented. The documented source is a dispatch source of
//! the type `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`, which needs code that is
//! not safe Rust: issue #100.

use std::io;

use super::policy::Pressure;

/// The pressure that wakes the daemon: processes waited for memory 10 % of
/// the time. Then the host reclaims memory and starts to slow down, before
/// it swaps a large part of the memory of a process.
pub(crate) const PRESSURE: Pressure = Pressure {
    stall: std::time::Duration::from_millis(200),
    window: std::time::Duration::from_secs(2),
};

/// The source of the pressure reports.
#[derive(Debug)]
pub(crate) struct PressureReports {
    #[cfg(target_os = "linux")]
    trigger: tokio::io::unix::AsyncFd<std::fs::File>,
}

impl PressureReports {
    /// Makes the trigger.
    ///
    /// # Errors
    ///
    /// Returns an error if the operating system gives no pressure reports:
    /// a kernel without PSI, or macOS.
    #[cfg(target_os = "linux")]
    pub(crate) fn open() -> io::Result<Self> {
        use std::io::Write;
        use tokio::io::Interest;

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/proc/pressure/memory")?;
        let trigger = format!(
            "some {} {}\0",
            PRESSURE.stall.as_micros(),
            PRESSURE.window.as_micros()
        );
        file.write_all(trigger.as_bytes())?;
        // The reactor of the daemon waits on the trigger, thus the daemon
        // keeps one thread. The function is deprecated because a value that
        // gives its descriptor can close it while the reactor has it; this
        // `File` is owned here and closes only when the `AsyncFd` is dropped.
        #[expect(deprecated, reason = "the safe replacement needs unsafe code")]
        let trigger = tokio::io::unix::AsyncFd::with_interest(file, Interest::PRIORITY)?;
        Ok(Self { trigger })
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn open() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this operating system gives the daemon no memory pressure reports",
        ))
    }

    /// Waits for the next report.
    ///
    /// # Errors
    ///
    /// Returns an error if the trigger fails.
    #[cfg(target_os = "linux")]
    pub(crate) async fn next(&mut self) -> io::Result<Pressure> {
        let mut ready = self.trigger.ready(tokio::io::Interest::PRIORITY).await?;
        ready.clear_ready();
        Ok(PRESSURE)
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) async fn next(&mut self) -> io::Result<Pressure> {
        std::future::pending().await
    }
}
