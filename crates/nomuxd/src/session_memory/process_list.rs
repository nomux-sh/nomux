//! The process IDs of the processes that run on the host.

use std::io;

/// The process ID of each process that this user can see.
#[cfg(target_os = "linux")]
pub(crate) fn pids() -> io::Result<Vec<u32>> {
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        // `/proc` also has entries that are not processes.
        if let Some(pid) = entry?
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        {
            pids.push(pid);
        }
    }
    Ok(pids)
}

/// The process ID of each process that runs on the host.
#[cfg(target_os = "macos")]
#[expect(unsafe_code)]
pub(crate) fn pids() -> io::Result<Vec<u32>> {
    /// Processes that start between the two calls.
    const SPARE_ENTRIES: usize = 64;

    // SAFETY: without a buffer, the call writes nothing and returns the
    // number of processes.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let Ok(count @ 1..) = usize::try_from(count) else {
        return Err(io::Error::last_os_error());
    };
    let mut buffer: Vec<libc::c_int> = vec![0; count + SPARE_ENTRIES];
    let size = libc::c_int::try_from(size_of_val(buffer.as_slice()))
        .map_err(|_| io::Error::other("the process table is too large"))?;
    // SAFETY: `buffer` is a writable buffer of `size` bytes, and the call
    // writes `size` bytes at most.
    let written = unsafe { libc::proc_listallpids(buffer.as_mut_ptr().cast(), size) };
    let Ok(written @ 1..) = usize::try_from(written) else {
        return Err(io::Error::last_os_error());
    };
    buffer.truncate(written);
    Ok(buffer
        .into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .collect())
}
