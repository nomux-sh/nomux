//! The install of the hooks of a tool in its settings file, on this
//! computer: `hooks install`, `hooks uninstall` and `hooks status`.
//!
//! The adapter of the tool describes the entries as data
//! ([`HookInstallPlan`]). This module computes the new content of the file
//! and its unified diff, and writes the file only in
//! [`SettingsChange::apply`]. It never wraps or replaces the command of a
//! tool, and it adds only the entries of the plan.
//!
//! - `settings_path`: the path of the file on this platform.
//! - `settings_document`: the JSON document and its entries.
//! - `install_record`: what an uninstall needs.
//! - `settings_diff`: the unified diff.
//! - `settings_file`: the write of the file.

mod install_record;
mod settings_diff;
mod settings_document;
mod settings_file;
mod settings_path;

use std::path::{Path, PathBuf};

use install_record::{InstallRecord, Original, RecordStore, read_optional, recorded_tools};
use settings_document::{HookEntry, add, contains, entries_of, parse, remove, render};

use crate::bootstrap::sha256_hex;
use crate::{ClientError, HookInstallPlan};

/// What an install or an uninstall of hooks changes in a settings file.
#[derive(Debug)]
pub struct SettingsChange {
    /// The settings file.
    pub path: PathBuf,
    /// The change as a unified diff. Empty if nothing changes.
    pub diff: String,
    /// `true` for an uninstall that gives the file its bytes from before
    /// the install. `false` for an uninstall that removes only the entries
    /// of nomux, because the file changed after the install.
    pub restores_original: bool,
    before: Option<Vec<u8>>,
    /// `None`: the file does not exist after the change.
    after: Option<Vec<u8>>,
    record: RecordChange,
}

#[derive(Debug)]
enum RecordChange {
    Keep,
    Save {
        record: InstallRecord,
        original: Option<Vec<u8>>,
    },
    Remove,
}

/// The state of the hooks of a tool in its settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HooksState {
    /// The file has each entry.
    Installed,
    /// The file has no entry.
    Missing,
    /// The file has some entries and not all.
    Different,
}

/// The hooks of one tool on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHooks {
    /// The settings file.
    pub path: PathBuf,
    /// The state of the hooks in the file.
    pub state: HooksState,
    /// `true` if nomux has a record of an install: `hooks uninstall` can
    /// remove the hooks.
    pub recorded: bool,
}

impl SettingsChange {
    /// `true` if the change changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.before == self.after
    }

    /// Writes the change: the settings file and the record of the install.
    ///
    /// # Errors
    ///
    /// Returns an error if a file cannot be written.
    pub fn apply(self, tool: &str) -> Result<(), ClientError> {
        let store = RecordStore::of(tool)?;
        if let RecordChange::Save { record, original } = &self.record {
            store.save(record, original.as_deref())?;
        }
        if !self.is_empty() {
            settings_file::write(&self.path, self.after.as_deref())?;
        }
        if let RecordChange::Remove = self.record {
            store.remove()?;
        }
        Ok(())
    }
}

/// The change that the hooks of `tool` need in its settings file.
///
/// # Errors
///
/// Returns [`ClientError::Failed`] if the plan is not valid or the settings
/// file is not a JSON object.
pub fn plan_hook_install(
    tool: &str,
    plan: &HookInstallPlan,
) -> Result<SettingsChange, ClientError> {
    let path = settings_path::settings_path(plan)?;
    let entries = entries_of(plan)?;
    let before = read_optional(&path)?;
    let mut document = document_of(&path, before.as_deref())?;
    if entries.iter().all(|entry| contains(&document, entry)) {
        return Ok(unchanged(path, before));
    }
    for entry in &entries {
        add(&mut document, entry).map_err(|problem| not_changeable(&path, &problem))?;
    }
    let after = render(&document);
    let store = RecordStore::of(tool)?;
    let earlier = store.load()?;
    let (original, original_bytes) = match &earlier {
        None => match &before {
            None => (Original::Absent, None),
            Some(bytes) => (Original::Saved, Some(bytes.clone())),
        },
        // The file is as the earlier install left it: its original stays.
        Some(earlier)
            if Some(&earlier.installed_sha256) == before.as_deref().map(sha256_hex).as_ref() =>
        {
            (earlier.original, None)
        }
        Some(_) => (Original::Unknown, None),
    };
    let record = InstallRecord {
        settings_path: path.clone(),
        installed_sha256: sha256_hex(&after),
        original,
        entries: union(earlier.map_or_default(|earlier| earlier.entries), entries),
    };
    Ok(SettingsChange {
        diff: settings_diff::unified_diff(&path, before.as_deref(), Some(&after)),
        path,
        restores_original: false,
        before,
        after: Some(after),
        record: RecordChange::Save {
            record,
            original: original_bytes,
        },
    })
}

/// The change that removes the hooks of `tool` that nomux installed.
///
/// # Errors
///
/// Returns [`ClientError::NotFound`] if nomux did not install the hooks of
/// the tool, and [`ClientError::Failed`] if the file is not a JSON object
/// any more.
pub fn plan_hook_uninstall(tool: &str) -> Result<SettingsChange, ClientError> {
    let store = RecordStore::of(tool)?;
    let record = store.load()?.ok_or_else(|| {
        ClientError::NotFound(format!("nomux did not install the hooks of {tool}"))
    })?;
    let path = record.settings_path.clone();
    let before = read_optional(&path)?;
    let unchanged_since_install =
        before.as_deref().map(sha256_hex).as_ref() == Some(&record.installed_sha256);
    let (after, restores_original) = match (unchanged_since_install, record.original) {
        (true, Original::Absent) => (None, true),
        (true, Original::Saved) => (Some(store.original()?), true),
        _ => (
            without_entries(&path, before.as_deref(), &record.entries)?,
            false,
        ),
    };
    Ok(SettingsChange {
        diff: settings_diff::unified_diff(&path, before.as_deref(), after.as_deref()),
        path,
        restores_original,
        before,
        after,
        record: RecordChange::Remove,
    })
}

/// The hooks of `tool`: from the plan of its adapter, or from the record of
/// an install if the tool has no adapter. `None` if there is neither.
///
/// # Errors
///
/// Returns [`ClientError::Failed`] if the plan is not valid or the settings
/// file is not a JSON object.
pub fn hooks_of_tool(
    tool: &str,
    plan: Option<&HookInstallPlan>,
) -> Result<Option<ToolHooks>, ClientError> {
    let record = RecordStore::of(tool)?.load()?;
    let (path, entries) = match (plan, &record) {
        (Some(plan), _) => (settings_path::settings_path(plan)?, entries_of(plan)?),
        (None, Some(record)) => (record.settings_path.clone(), record.entries.clone()),
        (None, None) => return Ok(None),
    };
    let content = read_optional(&path)?;
    let document = document_of(&path, content.as_deref())?;
    let present = entries
        .iter()
        .filter(|entry| contains(&document, entry))
        .count();
    let state = match present {
        0 => HooksState::Missing,
        present if present == entries.len() => HooksState::Installed,
        _ => HooksState::Different,
    };
    Ok(Some(ToolHooks {
        path,
        state,
        recorded: record.is_some(),
    }))
}

/// The tools for which nomux has a record of an install.
///
/// # Errors
///
/// Returns an error if the directory of the records cannot be read.
pub fn tools_with_installed_hooks() -> Result<Vec<String>, ClientError> {
    recorded_tools()
}

fn without_entries(
    path: &Path,
    before: Option<&[u8]>,
    entries: &[HookEntry],
) -> Result<Option<Vec<u8>>, ClientError> {
    let Some(content) = before else {
        return Ok(None);
    };
    let mut document = document_of(path, Some(content))?;
    if !entries.iter().any(|entry| contains(&document, entry)) {
        return Ok(Some(content.to_vec()));
    }
    for entry in entries {
        remove(&mut document, entry);
    }
    Ok(Some(render(&document)))
}

fn unchanged(path: PathBuf, before: Option<Vec<u8>>) -> SettingsChange {
    SettingsChange {
        path,
        diff: String::new(),
        restores_original: false,
        after: before.clone(),
        before,
        record: RecordChange::Keep,
    }
}

fn document_of(path: &Path, content: Option<&[u8]>) -> Result<serde_json::Value, ClientError> {
    parse(content.unwrap_or_default()).map_err(|problem| not_changeable(path, &problem))
}

fn not_changeable(path: &Path, problem: &str) -> ClientError {
    ClientError::Failed(format!(
        "the settings file {} cannot get the hooks: {problem}; nothing changed",
        path.display()
    ))
}

fn union(mut earlier: Vec<HookEntry>, entries: Vec<HookEntry>) -> Vec<HookEntry> {
    for entry in entries {
        if !earlier.contains(&entry) {
            earlier.push(entry);
        }
    }
    earlier
}
