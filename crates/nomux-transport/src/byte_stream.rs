//! The two directions of one connection, without the type of the transport.
//!
//! A connection is a Unix socket or an SSH channel. The framing code reads
//! and writes bytes and does not know which one it has.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The receiving direction of a connection.
pub(crate) struct ReadHalf(Box<dyn AsyncRead + Send + Unpin>);

impl ReadHalf {
    pub(crate) fn new(read: impl AsyncRead + Send + Unpin + 'static) -> Self {
        Self(Box::new(read))
    }
}

impl AsyncRead for ReadHalf {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(context, buffer)
    }
}

impl fmt::Debug for ReadHalf {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReadHalf")
    }
}

/// The sending direction of a connection. The peer sees the end of the
/// direction when this value is dropped.
pub(crate) struct WriteHalf(Box<dyn AsyncWrite + Send + Unpin>);

impl WriteHalf {
    pub(crate) fn new(write: impl AsyncWrite + Send + Unpin + 'static) -> Self {
        Self(Box::new(write))
    }
}

impl AsyncWrite for WriteHalf {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(context, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}

impl fmt::Debug for WriteHalf {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WriteHalf")
    }
}
