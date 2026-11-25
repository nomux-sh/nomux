//! Opens the pseudoterminal of the session.

use std::io;
use std::os::fd::{AsFd, OwnedFd};

use nomux_holder_protocol::wire::WindowSize;
use rustix::fs::{Mode, OFlags, fcntl_getfl, fcntl_setfl};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{Winsize, tcsetwinsize};

/// The two sides of a new pseudoterminal. Both descriptors are
/// close-on-exec.
pub(crate) struct Pty {
    /// The side of the holder. It is non-blocking.
    pub(crate) master: OwnedFd,
    /// The side of the child command.
    pub(crate) slave: OwnedFd,
}

pub(crate) fn open(size: WindowSize) -> io::Result<Pty> {
    let master = open_master()?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let slave_name = ptsname(&master, Vec::new())?;
    let slave = rustix::fs::open(
        slave_name,
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    resize(&slave, size)?;
    let flags = fcntl_getfl(&master)?;
    fcntl_setfl(&master, flags | OFlags::NONBLOCK)?;
    Ok(Pty { master, slave })
}

pub(crate) fn resize(terminal: impl AsFd, size: WindowSize) -> io::Result<()> {
    let size = Winsize {
        ws_row: size.rows,
        ws_col: size.columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    tcsetwinsize(terminal, size)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_master() -> io::Result<OwnedFd> {
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    Ok(openpt(flags)?)
}

/// `posix_openpt` has a close-on-exec flag only on Linux.
#[cfg(not(target_os = "linux"))]
fn open_master() -> io::Result<OwnedFd> {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
    rustix::io::fcntl_setfd(&master, rustix::io::FdFlags::CLOEXEC)?;
    Ok(master)
}
