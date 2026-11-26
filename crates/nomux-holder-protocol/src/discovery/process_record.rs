//! Reads the record of a process from the operating system: its start time,
//! its parent, the name of its program and the foreground process group
//! of its terminal.

use std::io;

/// What the operating system says about one process.
pub(super) struct ProcessRecord {
    /// The start time, in the unit of the operating system.
    pub(super) start_time: u64,
    /// The process ID of the parent.
    pub(super) parent_pid: u32,
    /// The name of the program. Linux gives 15 bytes at most, macOS gives
    /// 32 bytes at most.
    pub(super) name: String,
    /// The foreground process group of the controlling terminal of the
    /// process. `None` if the process has no controlling terminal, or if
    /// the terminal has no foreground process group.
    pub(super) terminal_foreground_group: Option<u32>,
}

/// The foreground process group of the controlling terminal of the process
/// `pid`: the process ID of the leader of the group. The daemon calls this
/// for the command of a session, whose controlling terminal is the terminal
/// of the session.
///
/// Returns `None` if no such process runs, if it has no controlling
/// terminal, or if the terminal has no foreground process group.
///
/// # Errors
///
/// Returns an error if the operating system does not give the record of a
/// process that exists.
pub fn terminal_foreground_group(pid: u32) -> io::Result<Option<u32>> {
    Ok(read(pid)?.and_then(|record| record.terminal_foreground_group))
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
/// is field 3, the parent is field 4, the foreground process group of the
/// terminal is field 8 and the start time is field 22 (`proc_pid_stat(5)`).
#[cfg(target_os = "linux")]
fn parse_stat(stat: &str) -> io::Result<Option<ProcessRecord>> {
    const FIELDS_FROM_PARENT_TO_FOREGROUND_GROUP: usize = 4;
    const FIELDS_FROM_FOREGROUND_GROUP_TO_START_TIME: usize = 14;

    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid /proc stat file");
    let (before_name_end, after_name) = stat.rsplit_once(')').ok_or_else(invalid)?;
    let (_, name) = before_name_end.split_once('(').ok_or_else(invalid)?;
    let mut fields = after_name.split_ascii_whitespace();
    let state = fields.next().ok_or_else(invalid)?;
    if state == "Z" || state == "X" {
        return Ok(None);
    }
    let parent_pid = fields
        .next()
        .and_then(|field| field.parse().ok())
        .ok_or_else(invalid)?;
    // The field is -1 for a process without a controlling terminal.
    let foreground_group: i64 = fields
        .nth(FIELDS_FROM_PARENT_TO_FOREGROUND_GROUP - 1)
        .and_then(|field| field.parse().ok())
        .ok_or_else(invalid)?;
    let start_time = fields
        .nth(FIELDS_FROM_FOREGROUND_GROUP_TO_START_TIME - 1)
        .and_then(|field| field.parse().ok())
        .ok_or_else(invalid)?;
    Ok(Some(ProcessRecord {
        start_time,
        parent_pid,
        name: name.to_owned(),
        terminal_foreground_group: u32::try_from(foreground_group)
            .ok()
            .filter(|group| *group != 0),
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
        parent_pid: info.pbi_ppid,
        name,
        // `e_tpgid` is 0 for a process without a controlling terminal.
        terminal_foreground_group: Some(info.e_tpgid).filter(|group| *group != 0),
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
        assert_eq!(record.parent_pid, 1);
        assert_eq!(record.name, "a) b) (c");
        assert_eq!(record.terminal_foreground_group, None);
    }

    #[test]
    fn the_foreground_group_of_the_terminal_is_field_8() {
        let stat = "4242 (sh) S 1 4242 4242 34816 4300 4194304 100 0 0 0 1 2 0 0 20 0 1 0 \
                    987654 1000000 200";
        let record = parse_stat(stat).unwrap().unwrap();
        assert_eq!(record.terminal_foreground_group, Some(4300));
        assert_eq!(record.start_time, 987_654);
    }

    #[test]
    fn a_zombie_is_not_a_process_that_runs() {
        let stat = "4242 (sh) Z 1 4242 4242 0 -1 4194304 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
        assert!(parse_stat(stat).unwrap().is_none());
    }
}
