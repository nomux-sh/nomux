//! The session metadata: what is necessary to start the command of a
//! session again, and how the session ended.
//!
//! The file is plain text, and a person can read and change it. Each line
//! is a key, and a value after one space:
//!
//! ```text
//! nomux-session 1
//! epoch 1
//! started 1795651200
//! rows 24
//! columns 80
//! restore on-focus
//! profile
//! directory /home/user/project
//! command /bin/sh
//! argument -c
//! argument echo hello
//! status running
//! ```
//!
//! | Key | Value |
//! |---|---|
//! | `nomux-session` | The format version: 1. It is the first line |
//! | `epoch` | The epoch of the holder that wrote the file |
//! | `started` | When that holder started: seconds after the Unix epoch |
//! | `rows`, `columns` | The size of the window now |
//! | `restore` | The restore policy: `on-focus` |
//! | `profile` | The name of the environment profile. Empty in this version |
//! | `directory` | The working directory of the command |
//! | `command` | The program of the command |
//! | `argument` | One argument of the command. One line for each argument, in their sequence |
//! | `status` | `running`, `exited <exit code>` or `signaled <signal number>` |
//! | `ended` | When the command ended: seconds after the Unix epoch. Only after the end |
//!
//! `directory`, `command` and `argument` use the escape sequences of
//! [`metadata_text`](super::metadata_text). A reader ignores a key that it
//! does not know: a later version adds keys.
//!
//! The file has no environment variable and no value of one. The
//! environment of a command can have secrets, and they must not reach the
//! disk.

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::str::FromStr;

use super::metadata_text::{escape, unescape};
use crate::wire::{ChildStatus, WindowSize};

const FORMAT_LINE: &str = "nomux-session 1";

/// When the daemon starts the command of a session again after the host
/// started again. A later version adds policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RestorePolicy {
    /// When a client opens the session for the first time.
    OnFocus,
}

/// The contents of the `session` file of a session state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMetadata {
    /// The epoch of the holder that wrote the file.
    pub epoch: u32,
    /// When that holder started: seconds after the Unix epoch.
    pub started: u64,
    /// The size of the window now.
    pub size: WindowSize,
    /// When the daemon starts the command again.
    pub restore_policy: RestorePolicy,
    /// The name of the environment profile of the command. Empty in this
    /// version: the file never has the environment.
    pub profile: String,
    /// The working directory of the command.
    pub working_directory: PathBuf,
    /// The program of the command.
    pub command: OsString,
    /// The arguments of the command.
    pub arguments: Vec<OsString>,
    /// If the command runs, or how it ended. A file that says `Running` for
    /// a session without a live holder is from a holder that was stopped.
    pub status: ChildStatus,
    /// When the command ended: seconds after the Unix epoch.
    pub ended: Option<u64>,
}

impl SessionMetadata {
    /// The text of the file.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut text = String::new();
        let mut line = |key: &str, value: &dyn fmt::Display| {
            let value = value.to_string();
            // A line does not end with a space: an editor can remove it.
            let separator = if value.is_empty() { "" } else { " " };
            // A write to a `String` does not fail.
            let _ = writeln!(text, "{key}{separator}{value}");
        };
        line("epoch", &self.epoch);
        line("started", &self.started);
        line("rows", &self.size.rows);
        line("columns", &self.size.columns);
        line("restore", &restore_policy_text(self.restore_policy));
        line("profile", &escape(self.profile.as_bytes()));
        line(
            "directory",
            &escape(self.working_directory.as_os_str().as_bytes()),
        );
        line("command", &escape(self.command.as_bytes()));
        for argument in &self.arguments {
            line("argument", &escape(argument.as_bytes()));
        }
        match self.status {
            ChildStatus::Running => line("status", &"running"),
            ChildStatus::Exited { code } => line("status", &format_args!("exited {code}")),
            ChildStatus::Signaled { signal } => line("status", &format_args!("signaled {signal}")),
        }
        if let Some(ended) = self.ended {
            line("ended", &ended);
        }
        format!("{FORMAT_LINE}\n{text}")
    }

    /// Reads the text of the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the first line is not the format line of this
    /// version, if a known key has a value that is not valid, or if a
    /// necessary key is missing.
    pub fn parse(text: &str) -> Result<Self, InvalidMetadata> {
        let mut lines = text.lines().enumerate();
        if lines.next().map(|(_, line)| line) != Some(FORMAT_LINE) {
            return Err(InvalidMetadata::Line(1));
        }
        let mut fields = Fields::default();
        for (index, line) in lines {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            fields
                .set(key, value)
                .ok_or(InvalidMetadata::Line(index + 1))?;
        }
        fields.complete()
    }
}

/// The values of the keys that the parser found.
#[derive(Default)]
struct Fields {
    epoch: Option<u32>,
    started: Option<u64>,
    rows: Option<u16>,
    columns: Option<u16>,
    restore_policy: Option<RestorePolicy>,
    profile: Option<String>,
    working_directory: Option<PathBuf>,
    command: Option<OsString>,
    arguments: Vec<OsString>,
    status: Option<ChildStatus>,
    ended: Option<u64>,
}

impl Fields {
    /// Keeps the value of one line. Returns `None` if the value is not
    /// valid for its key.
    fn set(&mut self, key: &str, value: &str) -> Option<()> {
        match key {
            "epoch" => self.epoch = Some(nonzero(value)?),
            "started" => self.started = Some(value.parse().ok()?),
            "rows" => self.rows = Some(nonzero(value)?),
            "columns" => self.columns = Some(nonzero(value)?),
            "restore" => self.restore_policy = Some(parse_restore_policy(value)?),
            "profile" => self.profile = Some(String::from_utf8(unescape(value)?).ok()?),
            "directory" => self.working_directory = Some(os_string(value)?.into()),
            "command" => self.command = Some(os_string(value)?),
            "argument" => self.arguments.push(os_string(value)?),
            "status" => self.status = Some(parse_status(value)?),
            "ended" => self.ended = Some(value.parse().ok()?),
            // A key of a later version.
            _ => {}
        }
        Some(())
    }

    fn complete(self) -> Result<SessionMetadata, InvalidMetadata> {
        let missing = InvalidMetadata::Missing;
        Ok(SessionMetadata {
            epoch: self.epoch.ok_or(missing("epoch"))?,
            started: self.started.ok_or(missing("started"))?,
            size: WindowSize {
                rows: self.rows.ok_or(missing("rows"))?,
                columns: self.columns.ok_or(missing("columns"))?,
            },
            restore_policy: self.restore_policy.unwrap_or(RestorePolicy::OnFocus),
            profile: self.profile.unwrap_or_default(),
            working_directory: self.working_directory.ok_or(missing("directory"))?,
            command: self.command.ok_or(missing("command"))?,
            arguments: self.arguments,
            status: self.status.unwrap_or(ChildStatus::Running),
            ended: self.ended,
        })
    }
}

fn nonzero<T: FromStr + Default + PartialEq>(value: &str) -> Option<T> {
    value.parse().ok().filter(|number| *number != T::default())
}

fn os_string(value: &str) -> Option<OsString> {
    unescape(value).map(OsString::from_vec)
}

fn restore_policy_text(policy: RestorePolicy) -> &'static str {
    match policy {
        RestorePolicy::OnFocus => "on-focus",
    }
}

fn parse_restore_policy(value: &str) -> Option<RestorePolicy> {
    (value == restore_policy_text(RestorePolicy::OnFocus)).then_some(RestorePolicy::OnFocus)
}

fn parse_status(value: &str) -> Option<ChildStatus> {
    if value == "running" {
        return Some(ChildStatus::Running);
    }
    let (state, number) = value.split_once(' ')?;
    let number = number.parse().ok()?;
    match state {
        "exited" => Some(ChildStatus::Exited { code: number }),
        "signaled" => Some(ChildStatus::Signaled { signal: number }),
        _ => None,
    }
}

/// The text is not the session metadata of this format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidMetadata {
    /// The line with this number, from 1, is not valid.
    Line(usize),
    /// The text does not have this key.
    Missing(&'static str),
}

impl fmt::Display for InvalidMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line(number) => {
                write!(
                    formatter,
                    "line {number} of the session metadata is not valid"
                )
            }
            Self::Missing(key) => write!(formatter, "the session metadata has no `{key}` line"),
        }
    }
}

impl Error for InvalidMetadata {}
