//! Reads the user ID of the process at the other end of a Unix socket.

use std::io;
use std::os::unix::net::UnixStream;

#[cfg(target_os = "linux")]
pub(super) fn peer_user_id(stream: &UnixStream) -> io::Result<u32> {
    let credentials = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(credentials.uid.as_raw())
}

/// `rustix` has no interface for the peer credentials on macOS.
#[cfg(target_os = "macos")]
#[expect(unsafe_code)]
pub(super) fn peer_user_id(stream: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;

    let mut user_id: libc::uid_t = 0;
    let mut group_id: libc::gid_t = 0;
    // SAFETY: the descriptor is open for the full call, because `stream` is
    // borrowed. The two pointers are valid for a write of one value each.
    let result =
        unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut user_id, &raw mut group_id) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(user_id)
}
