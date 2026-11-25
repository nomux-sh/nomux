//! Reads the start time of a process from the operating system.

use std::io;

/// The start time of the process `pid`. Returns `None` if no such process
/// runs: the process ID is free, the process is a zombie, or the process is
/// not visible to this user.
#[cfg(target_os = "linux")]
pub(super) fn process_start_time(pid: u32) -> io::Result<Option<u64>> {
    // `ESRCH`: the process ended between `open` and `read`.
    const NO_SUCH_PROCESS: i32 = 3;

    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => parse_stat(&stat),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) if error.raw_os_error() == Some(NO_SUCH_PROCESS) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Parses `/proc/<pid>/stat`. The second field is the command name in
/// parentheses, and it can contain spaces and parentheses, thus the other
/// fields are counted after the last `)`. The state is field 3 and the start
/// time is field 22 (`proc_pid_stat(5)`).
#[cfg(target_os = "linux")]
fn parse_stat(stat: &str) -> io::Result<Option<u64>> {
    const FIELDS_FROM_STATE_TO_START_TIME: usize = 19;

    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid /proc stat file");
    let (_, after_name) = stat.rsplit_once(')').ok_or_else(invalid)?;
    let mut fields = after_name.split_ascii_whitespace();
    let state = fields.next().ok_or_else(invalid)?;
    if state == "Z" || state == "X" {
        return Ok(None);
    }
    fields
        .nth(FIELDS_FROM_STATE_TO_START_TIME - 1)
        .and_then(|field| field.parse().ok())
        .map(Some)
        .ok_or_else(invalid)
}

/// The start time of the process `pid`. Returns `None` if no such process
/// runs: the process ID is free, the process is a zombie, or the process is
/// not visible to this user.
#[cfg(target_os = "macos")]
#[expect(unsafe_code)]
pub(super) fn process_start_time(pid: u32) -> io::Result<Option<u64>> {
    use std::mem::MaybeUninit;

    const MICROSECONDS_IN_SECOND: u64 = 1_000_000;

    let Ok(pid) = libc::c_int::try_from(pid) else {
        return Ok(None);
    };
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>())
        .expect("the size of proc_bsdinfo fits in an int");
    // SAFETY: `info` is a writable buffer of `size` bytes, and
    // `PROC_PIDTBSDINFO` writes a `proc_bsdinfo` of `size` bytes at most.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ESRCH | libc::EPERM) => Ok(None),
            _ => Err(error),
        };
    }
    // SAFETY: `proc_pidinfo` returned `size`, thus it wrote the full struct.
    // The buffer was zeroed, and each bit pattern is a valid `proc_bsdinfo`.
    let info = unsafe { info.assume_init() };
    if info.pbi_status == libc::SZOMB {
        return Ok(None);
    }
    Ok(Some(
        info.pbi_start_tvsec * MICROSECONDS_IN_SECOND + info.pbi_start_tvusec,
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_stat;

    #[test]
    fn a_command_name_with_spaces_and_parentheses_does_not_move_the_fields() {
        let stat = "4242 (a) b) (c) S 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 \
                    987654 1000000 200 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0";
        assert_eq!(parse_stat(stat).unwrap(), Some(987_654));
    }

    #[test]
    fn a_zombie_is_not_a_process_that_runs() {
        let stat = "4242 (sh) Z 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
        assert_eq!(parse_stat(stat).unwrap(), None);
    }
}
