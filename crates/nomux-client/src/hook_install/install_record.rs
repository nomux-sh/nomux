//! The record of a hook install: what `hooks uninstall` needs to restore a
//! settings file.
//!
//! The record of a tool is the directory `hooks/<tool>` in the data
//! directory of nomux, private to the user: `record.json`, and `original`
//! with the bytes of the settings file before the install.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use nomux_core::data_directory;
use serde::{Deserialize, Serialize};

use super::settings_document::HookEntry;
use super::settings_file::failure;
use crate::ClientError;

const RECORD_FILE: &str = "record.json";
const ORIGINAL_FILE: &str = "original";
const OWNER_ONLY: u32 = 0o700;

/// What a hook install did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct InstallRecord {
    /// The settings file.
    pub(super) settings_path: PathBuf,
    /// The SHA-256 checksum of the file that the install wrote.
    pub(super) installed_sha256: String,
    /// How the file was before the install.
    pub(super) original: Original,
    /// The entries that the install added.
    pub(super) entries: Vec<HookEntry>,
    /// The JSON pointers of the objects and the arrays that the install
    /// made. An uninstall of a changed file removes such a value when it has
    /// nothing else. `None` in a record of an earlier version of nomux: the
    /// uninstall removes each object and array that has nothing else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) made: Option<Vec<String>>,
}

/// The settings file before the install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Original {
    /// The file did not exist.
    Absent,
    /// The bytes of the file are in the file `original` of the record.
    Saved,
    /// The file changed after an earlier install: the bytes before the
    /// first install do not describe it any more. A removal removes the
    /// entries.
    Unknown,
}

/// The record directory of one tool.
#[derive(Debug)]
pub(super) struct RecordStore {
    directory: PathBuf,
}

impl RecordStore {
    /// The record directory of `tool`. The caller checked that `tool` is a
    /// tool name, thus it is one part of a path.
    pub(super) fn of(tool: &str) -> Result<Self, ClientError> {
        Ok(Self {
            directory: hooks_directory()?.join(tool),
        })
    }

    /// The record. `None` if nomux did not install the hooks of the tool.
    pub(super) fn load(&self) -> Result<Option<InstallRecord>, ClientError> {
        let path = self.directory.join(RECORD_FILE);
        let Some(text) = read_optional(&path)? else {
            return Ok(None);
        };
        serde_json::from_slice(&text).map(Some).map_err(|error| {
            ClientError::Failed(format!(
                "the hook install record {} is not valid: {error}",
                path.display()
            ))
        })
    }

    /// The bytes of the settings file before the install.
    pub(super) fn original(&self) -> Result<Vec<u8>, ClientError> {
        let path = self.directory.join(ORIGINAL_FILE);
        fs::read(&path).map_err(|error| failure(&path, &error))
    }

    /// Writes `record`, and `original` if the record saves the bytes.
    pub(super) fn save(
        &self,
        record: &InstallRecord,
        original: Option<&[u8]>,
    ) -> Result<(), ClientError> {
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(&self.directory)
            .map_err(|error| failure(&self.directory, &error))?;
        if let Some(original) = original {
            let path = self.directory.join(ORIGINAL_FILE);
            fs::write(&path, original).map_err(|error| failure(&path, &error))?;
        }
        let text = serde_json::to_vec_pretty(record).expect("a record can be written as JSON");
        let path = self.directory.join(RECORD_FILE);
        fs::write(&path, text).map_err(|error| failure(&path, &error))
    }

    /// Removes the record.
    pub(super) fn remove(&self) -> Result<(), ClientError> {
        match fs::remove_dir_all(&self.directory) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(failure(&self.directory, &error))
            }
            _ => Ok(()),
        }
    }
}

/// The tools that have a record, in the sequence of their names.
pub(super) fn recorded_tools() -> Result<Vec<String>, ClientError> {
    let directory = hooks_directory()?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(failure(&directory, &error)),
    };
    let mut tools: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(RECORD_FILE).is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    tools.sort();
    Ok(tools)
}

/// The content of the file at `path`. `None` if the file does not exist.
pub(super) fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ClientError> {
    match fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failure(path, &error)),
    }
}

fn hooks_directory() -> Result<PathBuf, ClientError> {
    let data = data_directory().ok_or_else(|| {
        ClientError::Failed(
            "the HOME variable is not set, thus the hook install records have no directory"
                .to_owned(),
        )
    })?;
    Ok(data.join("hooks"))
}
