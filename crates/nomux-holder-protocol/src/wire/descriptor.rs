//! Sends and receives bytes together with one file descriptor.
//!
//! The holder uses this to give the read-only descriptor of the ring buffer
//! to the daemon with the hello.

use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

/// Sends `bytes` on a Unix socket with `descriptor` as ancillary data.
///
/// # Errors
///
/// Returns the error of `sendmsg`.
pub fn send_with_descriptor(
    socket: impl AsFd,
    bytes: &[u8],
    descriptor: BorrowedFd<'_>,
) -> io::Result<usize> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let descriptors = [descriptor];
    if !control.push(SendAncillaryMessage::ScmRights(&descriptors)) {
        return Err(io::Error::other("no space for the descriptor"));
    }
    let sent = sendmsg(
        socket,
        &[IoSlice::new(bytes)],
        &mut control,
        SendFlags::empty(),
    )?;
    Ok(sent)
}

/// Receives bytes from a Unix socket, and the first descriptor that came
/// with them. The descriptor is close-on-exec.
///
/// # Errors
///
/// Returns the error of `recvmsg` or of `fcntl`.
pub fn receive_with_descriptor(
    socket: impl AsFd,
    buffer: &mut [u8],
) -> io::Result<(usize, Option<OwnedFd>)> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let received = recvmsg(
        socket,
        &mut [IoSliceMut::new(buffer)],
        &mut control,
        receive_flags(),
    )?;
    let descriptor = control
        .drain()
        .find_map(|message| match message {
            RecvAncillaryMessage::ScmRights(mut descriptors) => descriptors.next(),
            _ => None,
        })
        .map(close_on_exec)
        .transpose()?;
    Ok((received.bytes, descriptor))
}

#[cfg(target_os = "linux")]
fn receive_flags() -> RecvFlags {
    RecvFlags::CMSG_CLOEXEC
}

#[cfg(not(target_os = "linux"))]
fn receive_flags() -> RecvFlags {
    RecvFlags::empty()
}

#[cfg(target_os = "linux")]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the same signature as on the other hosts"
)]
fn close_on_exec(descriptor: OwnedFd) -> io::Result<OwnedFd> {
    Ok(descriptor)
}

/// Only Linux can set the flag in `recvmsg`.
#[cfg(not(target_os = "linux"))]
fn close_on_exec(descriptor: OwnedFd) -> io::Result<OwnedFd> {
    rustix::io::fcntl_setfd(&descriptor, rustix::io::FdFlags::CLOEXEC)?;
    Ok(descriptor)
}
