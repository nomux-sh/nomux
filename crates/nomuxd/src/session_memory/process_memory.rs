//! The memory that one process uses, from the operating system.

/// The memory of one process in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProcessMemory {
    /// The resident set size.
    pub(super) resident: u64,
    /// The proportional set size. `None` if the operating system does not
    /// give it for this process.
    pub(super) proportional: Option<u64>,
}

/// The memory of the process `pid`. Returns `None` if the process has no
/// memory that this user can read: the process ended, or it belongs to a
/// different user and the operating system does not show its memory.
#[cfg(target_os = "linux")]
pub(super) fn read(pid: u32) -> Option<ProcessMemory> {
    use std::fs;

    // `smaps_rollup` has the two values. The kernel refuses it for a process
    // that this user must not examine, for example a `setuid` program.
    // `status` has the resident set size of each process.
    fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .ok()
        .and_then(|rollup| parse_rollup(&rollup))
        .or_else(|| parse_status(&fs::read_to_string(format!("/proc/{pid}/status")).ok()?))
}

#[cfg(target_os = "linux")]
fn parse_rollup(rollup: &str) -> Option<ProcessMemory> {
    Some(ProcessMemory {
        resident: kibibyte_field(rollup, "Rss:")?,
        proportional: Some(kibibyte_field(rollup, "Pss:")?),
    })
}

#[cfg(target_os = "linux")]
fn parse_status(status: &str) -> Option<ProcessMemory> {
    Some(ProcessMemory {
        resident: kibibyte_field(status, "VmRSS:")?,
        proportional: None,
    })
}

/// The value of the line `<name> <number> kB` of a `/proc` file, in bytes.
#[cfg(target_os = "linux")]
fn kibibyte_field(text: &str, name: &str) -> Option<u64> {
    let value = text.lines().find_map(|line| line.strip_prefix(name))?;
    let kibibytes: u64 = value.split_ascii_whitespace().next()?.parse().ok()?;
    kibibytes.checked_mul(1024)
}

/// The memory of the process `pid`. Returns `None` if the process has no
/// memory that this user can read: the process ended, or it belongs to a
/// different user. macOS gives no proportional set size.
#[cfg(target_os = "macos")]
#[expect(unsafe_code)]
pub(super) fn read(pid: u32) -> Option<ProcessMemory> {
    use std::mem::MaybeUninit;

    let pid = libc::c_int::try_from(pid).ok()?;
    let mut info = MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = libc::c_int::try_from(size_of::<libc::proc_taskinfo>())
        .expect("the size of proc_taskinfo fits in an int");
    // SAFETY: `info` is a writable buffer of `size` bytes, and
    // `PROC_PIDTASKINFO` writes a `proc_taskinfo` of `size` bytes at most.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: `proc_pidinfo` returned `size`, thus it wrote the full struct.
    // The buffer was zeroed, and each bit pattern is a valid `proc_taskinfo`.
    let info = unsafe { info.assume_init() };
    Some(ProcessMemory {
        resident: info.pti_resident_size,
        proportional: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_has_memory_and_a_process_that_does_not_run_has_none() {
        let memory = read(std::process::id()).unwrap();

        assert!(memory.resident > 0);
        assert!(
            memory
                .proportional
                .is_none_or(|bytes| bytes <= memory.resident)
        );
        // The largest process ID of Linux and of macOS is smaller.
        assert_eq!(read(u32::MAX - 1), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_rollup_gives_the_two_values_in_bytes_and_the_status_only_the_resident_one() {
        let rollup = "55ea7071a000-7fffc992c000 ---p 00000000 00:00 0   [rollup]\n\
                      Rss:                2200 kB\n\
                      Pss:                 207 kB\n\
                      Pss_Dirty:           124 kB\n\
                      Pss_Anon:            124 kB\n";
        assert_eq!(
            parse_rollup(rollup),
            Some(ProcessMemory {
                resident: 2200 * 1024,
                proportional: Some(207 * 1024),
            })
        );

        let status = "Name:\tsh\nVmHWM:\t    9000 kB\nVmRSS:\t    6512 kB\nRssAnon:\t 100 kB\n";
        assert_eq!(
            parse_status(status),
            Some(ProcessMemory {
                resident: 6512 * 1024,
                proportional: None,
            })
        );
        // A kernel thread has no memory of its own.
        assert_eq!(
            parse_status("Name:\tkthreadd\nState:\tS (sleeping)\n"),
            None
        );
    }
}
