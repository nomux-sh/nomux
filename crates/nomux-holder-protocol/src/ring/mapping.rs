//! The memory mapping of a ring buffer region. All the `unsafe` code of the
//! ring buffer is in this file. Each function checks its range, thus the
//! callers are safe code.

use std::ffi::c_void;
use std::io;
use std::os::fd::BorrowedFd;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};

#[derive(Debug)]
pub(super) struct Mapping {
    base: NonNull<u8>,
    length: usize,
    writable: bool,
}

impl Mapping {
    pub(super) fn read_only(memory: BorrowedFd<'_>, length: usize) -> io::Result<Self> {
        Self::map(memory, length, false)
    }

    pub(super) fn writable(memory: BorrowedFd<'_>, length: usize) -> io::Result<Self> {
        Self::map(memory, length, true)
    }

    #[expect(unsafe_code)]
    fn map(memory: BorrowedFd<'_>, length: usize, writable: bool) -> io::Result<Self> {
        let protection = if writable {
            ProtFlags::READ | ProtFlags::WRITE
        } else {
            ProtFlags::READ
        };
        // SAFETY: the kernel selects the address, thus the new mapping
        // replaces no memory that Rust uses. The mapping is shared with a
        // different process, and this module reads and writes it only through
        // raw pointers and atomics, which permit that.
        let address = unsafe {
            mmap(
                ptr::null_mut(),
                length,
                protection,
                MapFlags::SHARED,
                memory,
                0,
            )
        }?;
        let base = NonNull::new(address.cast::<u8>())
            .ok_or_else(|| io::Error::other("mmap returned a null address"))?;
        Ok(Self {
            base,
            length,
            writable,
        })
    }

    pub(super) fn load_u64(&self, offset: usize) -> u64 {
        self.atomic(offset).load(Ordering::Acquire)
    }

    pub(super) fn store_u64(&mut self, offset: usize, value: u64) {
        assert!(self.writable, "the mapping is read-only");
        self.atomic(offset).store(value, Ordering::Release);
    }

    #[expect(unsafe_code)]
    fn atomic(&self, offset: usize) -> &AtomicU64 {
        self.check_range(offset, size_of::<AtomicU64>());
        assert!(offset.is_multiple_of(align_of::<AtomicU64>()));
        // SAFETY: the range is inside the mapping. The mapping starts on a
        // page boundary and the offset is a multiple of the alignment, thus
        // the pointer is aligned. The mapping lives as long as `self`. All
        // processes access these 8 bytes only as an atomic.
        unsafe { self.base.add(offset).cast::<AtomicU64>().as_ref() }
    }

    /// Copies bytes out of the mapping. A different process can change the
    /// bytes during the copy: the caller must validate the result.
    #[expect(unsafe_code)]
    pub(super) fn copy_out(&self, offset: usize, destination: &mut [u8]) {
        self.check_range(offset, destination.len());
        // SAFETY: the source range is inside the mapping and the destination
        // is a separate Rust slice of the same length. The copy reads through
        // a raw pointer and makes no reference to the shared bytes.
        unsafe {
            ptr::copy_nonoverlapping(
                self.base.add(offset).as_ptr(),
                destination.as_mut_ptr(),
                destination.len(),
            );
        }
    }

    /// A part of the mapping that this process can write. Only the one
    /// process that owns the writable mapping calls this.
    #[expect(unsafe_code)]
    pub(super) fn slice_mut(&mut self, offset: usize, length: usize) -> &mut [u8] {
        assert!(self.writable, "the mapping is read-only");
        self.check_range(offset, length);
        // SAFETY: the range is inside the writable mapping, and `&mut self`
        // makes this the only reference to it in this process. The other
        // processes have read-only mappings and read through raw pointers.
        unsafe { std::slice::from_raw_parts_mut(self.base.add(offset).as_ptr(), length) }
    }

    fn check_range(&self, offset: usize, length: usize) {
        let end = offset.checked_add(length);
        assert!(
            end.is_some_and(|end| end <= self.length),
            "range {offset}+{length} is outside the mapping of {} bytes",
            self.length
        );
    }
}

impl Drop for Mapping {
    #[expect(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: `base` and `length` are the result of the `mmap` call in
        // `map`, and no reference into the mapping lives longer than `self`.
        let unmapped = unsafe { munmap(self.base.as_ptr().cast::<c_void>(), self.length) };
        debug_assert!(unmapped.is_ok(), "munmap failed: {unmapped:?}");
    }
}
