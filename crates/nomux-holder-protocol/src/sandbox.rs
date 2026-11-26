//! The sandbox of a session: the features that the operating system can
//! apply, and the plan that the daemon gives a holder.
//!
//! The daemon resolves a sandbox profile into a [`SandboxPlan`] of
//! absolute paths and writes it to the file [`plan_file`] with mode 0600.
//! The holder gets the path with `--sandbox-plan`, reads the file, and
//! applies the plan to its command before the command starts. The file is
//! removed before the command starts, thus the command cannot read it. The
//! hello of the holder has the [`FeatureSet`] that the operating system
//! applies. `docs/sandbox.md` has the rules.
//!
//! The plan file is plain text, one rule on each line:
//!
//! ```text
//! nomux-sandbox-plan 1
//! profile project
//! read /usr
//! write /home/user/project
//! temporary /home/user/.local/state/nomux/sessions/k3xq2abc/tmp
//! unix /home/user/.nomux/run/nomuxd.agent
//! network off
//! require files
//! want unix_sockets
//! ```
//!
//! | Key | Value |
//! |---|---|
//! | `nomux-sandbox-plan` | The format version: 1. It is the first line |
//! | `profile` | The name of the profile |
//! | `read` | A path that the command can read and run, with each file below it |
//! | `write` | A path that the command can read, run, change, make and remove, with each file below it |
//! | `temporary` | The temporary directory of the session: the command can change it, and it is the `TMPDIR` of the command |
//! | `unix` | A path below which the command can connect to a Unix socket |
//! | `network` | `off`: the command cannot use the network. Without the line, the network is open |
//! | `require` | A feature that the operating system must apply, or the command does not start |
//! | `want` | A feature that the operating system applies if it can |
//!
//! A path has the escape sequences of the session metadata. A reader
//! refuses a key that it does not know: a plan that the holder does not
//! understand must not start a command with fewer limits.

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::SessionId;
use crate::session_state::metadata_text::{escape, unescape};

const FORMAT_LINE: &str = "nomux-sandbox-plan 1";

/// The plan file of the session `session_id` in the runtime directory. The
/// name has a `.`, thus it is not a session identifier. The daemon writes
/// it before each start of a holder of a sandboxed session.
#[must_use]
pub fn plan_file(runtime_directory: &Path, session_id: &SessionId) -> PathBuf {
    runtime_directory.join(format!("{session_id}.sandbox-plan"))
}

/// One limit that the operating system applies to the processes of a
/// sandboxed session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxFeature {
    /// The processes can read, write and run files only where the plan
    /// permits it.
    Files,
    /// The processes cannot open a socket of an internet protocol.
    NetworkOff,
    /// The processes can connect to a Unix socket only below a `unix` path.
    UnixSockets,
    /// The processes cannot send a signal to a process outside the sandbox.
    Signals,
    /// The processes cannot connect to an abstract Unix socket of a process
    /// outside the sandbox.
    AbstractSockets,
}

impl SandboxFeature {
    /// Each feature, in the sequence of their bits.
    pub const ALL: [Self; 5] = [
        Self::Files,
        Self::NetworkOff,
        Self::UnixSockets,
        Self::Signals,
        Self::AbstractSockets,
    ];

    /// The name of the feature in the plan, in the protocol and in the
    /// output of the CLI.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::NetworkOff => "network_off",
            Self::UnixSockets => "unix_sockets",
            Self::Signals => "signals",
            Self::AbstractSockets => "abstract_sockets",
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

impl FromStr for SandboxFeature {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, ()> {
        Self::ALL
            .into_iter()
            .find(|feature| feature.name() == name)
            .ok_or(())
    }
}

/// A set of features. The hello of a holder has it as one byte: bit 0 is
/// `files`, in the sequence of [`SandboxFeature::ALL`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeatureSet(u8);

impl FeatureSet {
    /// No feature.
    pub const EMPTY: Self = Self(0);

    /// The set from its byte. A bit of a later version is kept, and
    /// [`features`](Self::features) does not give it.
    #[must_use]
    pub fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// The byte of the hello.
    #[must_use]
    pub fn bits(self) -> u8 {
        self.0
    }

    /// The set with `feature`.
    #[must_use]
    pub fn with(self, feature: SandboxFeature) -> Self {
        Self(self.0 | feature.bit())
    }

    /// `true` if the set has `feature`.
    #[must_use]
    pub fn contains(self, feature: SandboxFeature) -> bool {
        self.0 & feature.bit() != 0
    }

    /// The features of `self` that `other` does not have.
    #[must_use]
    pub fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// The features of `self` and of `other`.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `true` if the set has no feature.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The known features of the set, in their sequence.
    pub fn features(self) -> impl Iterator<Item = SandboxFeature> {
        SandboxFeature::ALL
            .into_iter()
            .filter(move |feature| self.contains(*feature))
    }

    /// The names of the known features, for a person.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        self.features().map(SandboxFeature::name).collect()
    }
}

impl FromIterator<SandboxFeature> for FeatureSet {
    fn from_iter<I: IntoIterator<Item = SandboxFeature>>(features: I) -> Self {
        features.into_iter().fold(Self::EMPTY, Self::with)
    }
}

/// The concrete rules of the sandbox of one start of a session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxPlan {
    /// The name of the profile that the plan comes from.
    pub profile: String,
    /// The paths that the command can read and run.
    pub read: Vec<PathBuf>,
    /// The paths that the command can read, run and change.
    pub write: Vec<PathBuf>,
    /// The temporary directory of the session: the command can change it,
    /// and it is its `TMPDIR`.
    pub temporary: Option<PathBuf>,
    /// The paths below which the command can connect to a Unix socket.
    pub unix: Vec<PathBuf>,
    /// The command cannot use the network.
    pub network_off: bool,
    /// The features without which the command does not start.
    pub require: FeatureSet,
    /// The features that the holder applies if the operating system can.
    pub want: FeatureSet,
}

impl SandboxPlan {
    /// The text of the plan file.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut text = format!("{FORMAT_LINE}\n");
        // A write to a `String` does not fail.
        let _ = writeln!(text, "profile {}", escape(self.profile.as_bytes()));
        let temporary = self.temporary.iter().map(|path| ("temporary", path));
        let paths = (self.read.iter().map(|path| ("read", path)))
            .chain(self.write.iter().map(|path| ("write", path)))
            .chain(temporary)
            .chain(self.unix.iter().map(|path| ("unix", path)));
        for (key, path) in paths {
            let _ = writeln!(text, "{key} {}", escape(path.as_os_str().as_bytes()));
        }
        if self.network_off {
            text.push_str("network off\n");
        }
        for (key, set) in [("require", self.require), ("want", self.want)] {
            for name in set.names() {
                let _ = writeln!(text, "{key} {name}");
            }
        }
        text
    }

    /// Reads the text of a plan file.
    ///
    /// # Errors
    ///
    /// Returns the number of the first line that is not valid: the format
    /// line of another version, a key that this version does not know, a
    /// value that is not valid, or a path that is not absolute.
    pub fn parse(text: &str) -> Result<Self, InvalidPlan> {
        let mut lines = text.lines().enumerate();
        if lines.next().map(|(_, line)| line) != Some(FORMAT_LINE) {
            return Err(InvalidPlan { line: 1 });
        }
        let mut plan = Self::default();
        for (index, line) in lines {
            plan.set(line).ok_or(InvalidPlan { line: index + 1 })?;
        }
        Ok(plan)
    }

    fn set(&mut self, line: &str) -> Option<()> {
        let (key, value) = line.split_once(' ')?;
        match key {
            "profile" => self.profile = String::from_utf8(unescape(value)?).ok()?,
            "read" => self.read.push(absolute_path(value)?),
            "write" => self.write.push(absolute_path(value)?),
            "temporary" if self.temporary.is_none() => self.temporary = Some(absolute_path(value)?),
            "unix" => self.unix.push(absolute_path(value)?),
            "network" if value == "off" => self.network_off = true,
            "require" => self.require = self.require.with(value.parse().ok()?),
            "want" => self.want = self.want.with(value.parse().ok()?),
            _ => return None,
        }
        Some(())
    }
}

fn absolute_path(value: &str) -> Option<PathBuf> {
    Some(PathBuf::from(OsString::from_vec(unescape(value)?))).filter(|path| path.is_absolute())
}

/// The text is not a plan of this format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPlan {
    /// The number of the line, from 1.
    pub line: usize,
}

impl fmt::Display for InvalidPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "line {} of the sandbox plan is not valid for this holder",
            self.line
        )
    }
}

impl Error for InvalidPlan {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_comes_back_from_its_text() {
        let plan = SandboxPlan {
            profile: "project".to_owned(),
            read: vec!["/usr".into(), "/home/u/a b\nc".into()],
            write: vec!["/home/u/project".into()],
            temporary: Some("/home/u/.local/state/nomux/sessions/s/tmp".into()),
            unix: vec!["/home/u/.nomux/run/nomuxd.agent".into()],
            network_off: true,
            require: [SandboxFeature::Files, SandboxFeature::NetworkOff]
                .into_iter()
                .collect(),
            want: FeatureSet::EMPTY.with(SandboxFeature::UnixSockets),
        };
        assert_eq!(SandboxPlan::parse(&plan.encode()), Ok(plan));
    }

    #[test]
    fn a_key_or_a_feature_of_a_later_version_is_refused() {
        for (text, line) in [
            ("nomux-sandbox-plan 2\n", 1),
            ("nomux-sandbox-plan 1\nprofile p\nhide /home\n", 3),
            ("nomux-sandbox-plan 1\nrequire gpu\n", 2),
            ("nomux-sandbox-plan 1\nnetwork proxy\n", 2),
            ("nomux-sandbox-plan 1\nread relative/path\n", 2),
            ("nomux-sandbox-plan 1\ntemporary /a\ntemporary /b\n", 3),
        ] {
            assert_eq!(
                SandboxPlan::parse(text),
                Err(InvalidPlan { line }),
                "{text:?}"
            );
        }
    }
}
