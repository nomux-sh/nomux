//! Why the configuration file cannot be used.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The configuration file cannot be read, is not valid or cannot be
/// written. Each message has the path of the file.
#[derive(Debug)]
pub enum ConfigurationError {
    /// The file cannot be read.
    Read {
        /// The path of the file.
        path: PathBuf,
        /// The error of the operating system.
        source: io::Error,
    },
    /// The file is not TOML.
    Syntax {
        /// The path of the file.
        path: PathBuf,
        /// The message of the parser, with the line and the column.
        message: String,
    },
    /// A key has a value that is not valid, or the file does not know the
    /// key.
    InvalidKey {
        /// The path of the file.
        path: PathBuf,
        /// The key, for example `hosts.vps.ssh`.
        key: String,
        /// What is wrong with the key.
        problem: String,
    },
    /// The file cannot be written.
    Write {
        /// The path of the file.
        path: PathBuf,
        /// The error of the operating system.
        source: io::Error,
    },
}

impl ConfigurationError {
    pub(super) fn read(path: &Path, source: io::Error) -> Self {
        Self::Read {
            path: path.to_owned(),
            source,
        }
    }

    pub(super) fn syntax(path: &Path, error: &toml_edit::TomlError) -> Self {
        Self::Syntax {
            path: path.to_owned(),
            message: error.to_string(),
        }
    }

    pub(super) fn invalid_key(path: &Path, key: &str, problem: &str) -> Self {
        Self::InvalidKey {
            path: path.to_owned(),
            key: key.to_owned(),
            problem: problem.to_owned(),
        }
    }

    pub(super) fn write(path: &Path, source: io::Error) -> Self {
        Self::Write {
            path: path.to_owned(),
            source,
        }
    }
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(
                formatter,
                "the configuration file {} cannot be read: {source}",
                path.display()
            ),
            Self::Syntax { path, message } => write!(
                formatter,
                "the configuration file {} is not valid TOML: {}",
                path.display(),
                message.trim_end()
            ),
            Self::InvalidKey { path, key, problem } => write!(
                formatter,
                "the configuration file {} is not valid: the key `{key}` {problem}",
                path.display()
            ),
            Self::Write { path, source } => write!(
                formatter,
                "the configuration file {} cannot be written: {source}",
                path.display()
            ),
        }
    }
}

impl Error for ConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Syntax { .. } | Self::InvalidKey { .. } => None,
        }
    }
}
