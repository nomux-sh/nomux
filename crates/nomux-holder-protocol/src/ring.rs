//! The ring buffer that carries terminal output from a holder to the daemon.
//!
//! The ring buffer is one region of shared memory. The holder maps it for
//! writing ([`RingWriter`]). The daemon gets a read-only descriptor with the
//! hello and maps it for reading ([`RingReader`]). A read is a copy out of
//! the mapping: there is no system call and no copy in the kernel.
//!
//! # Layout
//!
//! ```text
//! offset  size  field
//!      0     8  magic: "NMXRING\0"
//!      8     4  layout version: 1
//!     12     4  data offset: where the data starts (64 in this version)
//!     16     8  capacity: the number of data bytes, a power of two
//!     24     8  reserved, zero
//!     32     8  claimed position (atomic)
//!     40     8  committed position (atomic)
//!     48    16  reserved, zero
//!     64     n  data, `capacity` bytes
//! ```
//!
//! All integers are little-endian. A reader uses the data offset from the
//! header, thus a later version can make the header longer.
//!
//! # Positions
//!
//! A position is the number of bytes that the holder wrote since it started.
//! It only increases. The byte at position `p` is at data index
//! `p mod capacity`.
//!
//! - The **committed** position is the end of the bytes that a reader can
//!   read.
//! - The **claimed** position is the end of the bytes that the writer can
//!   change. Before the writer writes a chunk, it sets the claimed position
//!   to the end of the chunk. After the write, it sets the committed
//!   position and the claimed position to the end of the bytes that it
//!   wrote. A chunk never goes over the end of the data: a write that reaches
//!   the end stops there and the next write starts at data index 0.
//!
//! The bytes from `claimed - capacity` to `committed` are stable.
//!
//! The arithmetic on positions wraps at 2^64. The capacity is a power of
//! two, thus the data index is correct after a wrap, and the positions of a
//! holder that is not trusted cannot make an addition of the reader overflow.
//!
//! # Overrun
//!
//! A reader keeps its own position. After it copies bytes that start at
//! position `r`, it loads the claimed position. If `claimed - r` is larger
//! than the capacity, the writer was able to change the bytes during the
//! copy: the reader was overrun. It discards the copy, continues at the
//! committed position and reports the number of bytes that it lost.

mod layout;
mod reader;
mod writer;

pub use layout::{HEADER_LENGTH, MIN_CAPACITY, region_length};
pub use reader::{Overrun, RingReader};
pub use writer::RingWriter;
