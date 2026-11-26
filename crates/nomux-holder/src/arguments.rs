//! The command line of the holder.

use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::session_state::default_state_directory;
use nomux_holder_protocol::wire::WindowSize;
use nomux_term::Size;

pub(crate) const USAGE: &str = "\
Usage: nomux-holder --session <identifier> [options] -- <command> [argument...]
       nomux-holder --version

Options:
  --runtime-directory <path>  Directory for the discovery files (default: ~/.nomux/run)
  --state-directory <path>    Directory for the session state: the journal and the
                              session metadata (default: the state directory of the user)
  --working-directory <path>  Working directory of the command (default: the current one)
  --rows <number>             Rows of the terminal window (default: 24)
  --columns <number>          Columns of the terminal window (default: 80)
  --ring-capacity <bytes>     Size of the ring buffer, a power of two (default: 262144)
  --exit-timeout <seconds>    Time to keep the exit status for the daemon (default: 30)
  --launcher <word>           A word of a program that starts the command, for
                              example a memory limit; repeat it for each word. The
                              holder starts the launcher with the command after its
                              words, and the session metadata has the command
  --pooled                    Start without a session: the daemon assigns the session
                              later. The standard input must be a pipe from the daemon:
                              a holder without a session ends when the pipe closes
  --directory-fifo <path>     With --pooled: open the named pipe at <path>. At the
                              assignment, the holder writes the working directory of
                              the session and a zero byte to it. The holder removes it
                              after the command read it, and at its end
  --start-argument <value>    One more argument for the command, after its arguments.
                              The session metadata does not have it. Can be repeated

The command gets the environment of the holder, and the variables
NOMUX_SESSION_ID, NOMUX_SOCK and NOMUX_TOKEN. NOMUX_TOKEN is the value of
NOMUX_HOLDER_TOKEN if the holder has that variable, else a new random token.
The command does not get NOMUX_HOLDER_TOKEN.
";

const DEFAULT_SIZE: WindowSize = WindowSize {
    rows: 24,
    columns: 80,
};
/// With this capacity, a holder whose ring buffer is full stays under the
/// memory target of `bench/README.md`.
const DEFAULT_RING_CAPACITY: usize = 256 * 1024;
const DEFAULT_EXIT_TIMEOUT: Duration = Duration::from_secs(30);

/// What the command line asks the holder to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation {
    Version,
    Session(Box<SessionArguments>),
}

/// The description of the session that the holder starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionArguments {
    pub(crate) session_id: SessionId,
    pub(crate) runtime_directory: PathBuf,
    pub(crate) state_directory: PathBuf,
    pub(crate) working_directory: Option<PathBuf>,
    pub(crate) size: WindowSize,
    pub(crate) ring_capacity: usize,
    pub(crate) exit_timeout: Duration,
    /// The holder waits in the pool of the daemon for its session.
    pub(crate) pooled: bool,
    /// The named pipe through which the command gets the working directory
    /// of the session at the assignment.
    pub(crate) directory_fifo: Option<PathBuf>,
    /// The program and the arguments that start the command. Empty if the
    /// holder starts the command itself.
    pub(crate) launcher: Vec<OsString>,
    pub(crate) program: OsString,
    pub(crate) program_arguments: Vec<OsString>,
    /// The arguments after `program_arguments` that only the start of the
    /// command has: the session metadata has the command without them.
    pub(crate) start_arguments: Vec<OsString>,
}

impl SessionArguments {
    /// The program that the holder starts and its arguments: the launcher
    /// with the command after it, or the command, and the start arguments.
    pub(crate) fn command_line(&self) -> Vec<&OsString> {
        self.launcher
            .iter()
            .chain([&self.program])
            .chain(&self.program_arguments)
            .chain(&self.start_arguments)
            .collect()
    }
}

pub(crate) fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Invocation, String> {
    let mut arguments = arguments.into_iter();
    let mut session_id = None;
    let mut runtime_directory = None;
    let mut state_directory = None;
    let mut working_directory = None;
    let mut size = DEFAULT_SIZE;
    let mut ring_capacity = DEFAULT_RING_CAPACITY;
    let mut exit_timeout = DEFAULT_EXIT_TIMEOUT;
    let mut pooled = false;
    let mut directory_fifo = None;
    let mut start_arguments = Vec::new();
    let mut launcher = Vec::new();

    loop {
        let Some(option) = arguments.next() else {
            return Err("the command is missing: put it after `--`".to_owned());
        };
        let Some(option) = option.to_str() else {
            return Err(format!("unknown option {}", option.display()));
        };
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{option} needs a value"))
        };
        match option {
            "--version" | "-V" => return Ok(Invocation::Version),
            "--" => break,
            "--pooled" => pooled = true,
            "--directory-fifo" => directory_fifo = Some(PathBuf::from(value()?)),
            "--start-argument" => start_arguments.push(value()?),
            "--launcher" => launcher.push(value()?),
            "--session" => session_id = Some(parse_value(option, &value()?)?),
            "--runtime-directory" => runtime_directory = Some(PathBuf::from(value()?)),
            "--state-directory" => state_directory = Some(PathBuf::from(value()?)),
            "--working-directory" => working_directory = Some(PathBuf::from(value()?)),
            "--rows" => size.rows = parse_value(option, &value()?)?,
            "--columns" => size.columns = parse_value(option, &value()?)?,
            "--ring-capacity" => ring_capacity = parse_value(option, &value()?)?,
            "--exit-timeout" => {
                exit_timeout = Duration::from_secs(parse_value(option, &value()?)?);
            }
            _ => return Err(format!("unknown option {option}")),
        }
    }

    let program = arguments
        .next()
        .ok_or("the command is missing after `--`")?;
    if directory_fifo.is_some() && !pooled {
        return Err("--directory-fifo needs --pooled".to_owned());
    }
    if size.rows == 0 {
        return Err("--rows must be larger than 0".to_owned());
    }
    if size.columns < Size::MIN_COLUMNS {
        return Err(format!("--columns must be {} or more", Size::MIN_COLUMNS));
    }
    if size.cells() > WindowSize::MAX_CELLS {
        return Err(format!(
            "--rows times --columns must be {} at most",
            WindowSize::MAX_CELLS
        ));
    }
    Ok(Invocation::Session(Box::new(SessionArguments {
        session_id: session_id.ok_or("--session is necessary")?,
        runtime_directory: runtime_directory
            .or_else(|| nomux_core::runtime_directory::locate().ok())
            .ok_or("--runtime-directory is necessary when HOME is not set")?,
        state_directory: state_directory
            .or_else(default_state_directory)
            .ok_or("--state-directory is necessary when HOME is not set")?,
        working_directory,
        size,
        ring_capacity,
        exit_timeout,
        pooled,
        directory_fifo,
        launcher,
        program,
        program_arguments: arguments.collect(),
        start_arguments,
    })))
}

fn parse_value<T>(option: &str, value: &OsString) -> Result<T, String>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let text = value
        .to_str()
        .ok_or_else(|| format!("the value of {option} is not valid text"))?;
    text.parse()
        .map_err(|error| format!("invalid value {text:?} for {option}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_text(arguments: &[&str]) -> Result<Invocation, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn the_arguments_after_the_separator_belong_to_the_command() {
        let invocation = parse_text(&[
            "--session",
            "s1",
            "--runtime-directory",
            "/run",
            "--rows",
            "50",
            "--",
            "sh",
            "--version",
            "--rows",
            "1",
        ])
        .unwrap();
        let Invocation::Session(session) = invocation else {
            panic!("not a session: {invocation:?}");
        };
        assert_eq!(session.size.rows, 50);
        assert_eq!(session.program, "sh");
        assert_eq!(session.program_arguments, ["--version", "--rows", "1"]);
    }

    #[test]
    fn a_window_of_1_column_is_rejected() {
        let result = parse_text(&["--session", "s1", "--columns", "1", "--", "sh"]);
        assert_eq!(result, Err("--columns must be 2 or more".to_owned()));
    }

    #[test]
    fn a_session_identifier_that_can_leave_the_runtime_directory_is_rejected() {
        let result = parse_text(&[
            "--session",
            "../x",
            "--runtime-directory",
            "/run",
            "--",
            "sh",
        ]);
        assert!(result.is_err(), "{result:?}");
    }
}
