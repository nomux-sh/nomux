//! Reads the record of a process from the operating system: its start time
//! and the name of its program.

use std::io;

/// What the operating system says about one process.
pub(super) struct ProcessRecord {
    /// The start time, in the unit of the operating system.
    pub(super) start_time: u64,
    /// The name of the program. Linux gives 15 bytes at most, macOS gives
    /// 32 bytes at most.
    pub(super) name: String,
}

/// The name of the program that the process `pid` runs: the last part of the
/// path that started it, or the name that the process gave itself. Linux
/// gives the first 15 bytes of the name, macOS the first 32 bytes.
///
/// Returns `None` if no such process runs: the process ID is free, the
/// process is a zombie, or the process is not visible to this user.
///
/// # Errors
///
/// Returns an error if the operating system does not give the record of a
/// process that exists.
pub fn program_name(pid: u32) -> io::Result<Option<String>> {
    Ok(read(pid)?.map(|record| record.name))
}

/// The record of the process `pid`. Returns `None` if no such process runs:
/// the process ID is free, the process is a zombie, or the process is not
/// visible to this user.
#[cfg(target_os = "linux")]
pub(super) fn read(pid: u32) -> io::Result<Option<ProcessRecord>> {
    // `ESRCH`: the process ended between `open` and `read`.
    const NO_SUCH_PROCESS: i32 = 3;

    match std::fs::read(format!("/proc/{pid}/stat")) {
        Ok(stat) => parse_stat(&String::from_utf8_lossy(&stat)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) if error.raw_os_error() == Some(NO_SUCH_PROCESS) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Parses `/proc/<pid>/stat`. The second field is the program name in
/// parentheses, and it can contain spaces and parentheses, thus the name
/// ends at the last `)` and the other fields are counted after it. The state
/// is field 3 and the start time is field 22 (`proc_pid_stat(5)`).
#[cfg(target_os = "linux")]
fn parse_stat(stat: &str) -> io::Result<Option<ProcessRecord>> {
    const FIELDS_FROM_STATE_TO_START_TIME: usize = 19;

    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid /proc stat file");
    let (before_name_end, after_name) = stat.rsplit_once(')').ok_or_else(invalid)?;
    let (_, name) = before_name_end.split_once('(').ok_or_else(invalid)?;
    let mut fields = after_name.split_ascii_whitespace();
    let state = fields.next().ok_or_else(invalid)?;
    if state == "Z" || state == "X" {
        return Ok(None);
    }
    let start_time = fields
        .nth(FIELDS_FROM_STATE_TO_START_TIME - 1)
        .and_then(|field| field.parse().ok())
        .ok_or_else(invalid)?;
    Ok(Some(ProcessRecord {
        start_time,
        name: name.to_owned(),
    }))
}

/// The record of the process `pid`. Returns `None` if no such process runs:
/// the process ID is free, the process is a zombie, or the process is not
/// visible to this user.
#[cfg(target_os = "macos")]
#[expect(unsafe_code)]
pub(super) fn read(pid: u32) -> io::Result<Option<ProcessRecord>> {
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
    // `pbi_name` has the long name. It is empty for a process that has only
    // the short name, as in `proc_name` of `libproc`.
    let name = if info.pbi_name[0] == 0 {
        name_field(&info.pbi_comm)
    } else {
        name_field(&info.pbi_name)
    };
    Ok(Some(ProcessRecord {
        start_time: info.pbi_start_tvsec * MICROSECONDS_IN_SECOND + info.pbi_start_tvusec,
        name,
    }))
}

/// The text of a name field: the bytes before the first NUL.
#[cfg(target_os = "macos")]
fn name_field(field: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = field
        .iter()
        .take_while(|character| **character != 0)
        .map(|character| character.to_ne_bytes()[0])
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_stat;

    #[test]
    fn a_program_name_with_spaces_and_parentheses_does_not_move_the_fields() {
        let stat = "4242 (a) b) (c) S 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 \
                    987654 1000000 200 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0";
        let record = parse_stat(stat).unwrap().unwrap();
        assert_eq!(record.start_time, 987_654);
        assert_eq!(record.name, "a) b) (c");
    }

    #[test]
    fn a_zombie_is_not_a_process_that_runs() {
        let stat = "4242 (sh) Z 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
        assert!(parse_stat(stat).unwrap().is_none());
    }
}
