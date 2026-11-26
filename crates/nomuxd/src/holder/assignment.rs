//! Makes a pooled holder the holder of a new session.

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use nomux_holder_protocol::wire::{Reply, Request};

use super::HolderLink;

/// Gives the pooled holder of `link` its session with `working_directory`.
/// A shell with a start file of nomux gets the directory from the holder.
/// `link` must have the lease.
pub(crate) async fn assign(link: &mut HolderLink, working_directory: &Path) -> io::Result<()> {
    let assignment = Request::AssignSession {
        size: link.hello.size,
        working_directory: working_directory.as_os_str().as_bytes(),
    };
    if !link.hello.protocol.supports(&assignment) {
        return Err(io::Error::other(
            "the holder is too old to get a session from the pool",
        ));
    }
    request(link, assignment).await
}

/// Sends one request that the holder answers with `Done`.
async fn request(link: &mut HolderLink, request: Request<'_>) -> io::Result<()> {
    link.connection.send(request).await?;
    match link.connection.receive().await? {
        (_, Reply::Done) => Ok(()),
        (_, reply) => Err(io::Error::other(format!(
            "the pooled holder refused {request:?}: {reply:?}"
        ))),
    }
}
