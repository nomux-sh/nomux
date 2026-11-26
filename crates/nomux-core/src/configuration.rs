//! The global configuration file of nomux: `config.toml`.
//!
//! This module reads only the parts of the file that the core uses now: the
//! hosts in `[hosts.<name>]`, `default_policy` in `[restore]` and
//! `ended_retention` in `[sessions]`. It writes only the hosts, and it keeps
//! each other part of the file, also the comments, when it writes the file.

mod duration_text;
mod error;
mod host_entry;
mod location;
mod restore_policy;

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use toml_edit::{DocumentMut, Item, Table, value};

pub use self::error::ConfigurationError;
pub use self::host_entry::{HostEntry, is_valid_host_name};
pub use self::location::default_configuration_path;
pub use self::restore_policy::{RestorePolicy, UnknownRestorePolicy};

const HOSTS: &str = "hosts";
const SSH: &str = "ssh";
const RESTORE: &str = "restore";
const DEFAULT_POLICY: &str = "default_policy";
const SESSIONS: &str = "sessions";
const ENDED_RETENTION: &str = "ended_retention";
const OWNER_ONLY_DIRECTORY: u32 = 0o700;
const OWNER_READ_WRITE: u32 = 0o600;

/// The global configuration file, in memory.
#[derive(Debug, Clone)]
pub struct ConfigurationFile {
    path: PathBuf,
    document: DocumentMut,
}

impl ConfigurationFile {
    /// Reads the file at `path` and validates the parts that this module
    /// knows. A file that does not exist is an empty configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, if it is not TOML, or
    /// if a key that this module knows is not valid. The error has the path
    /// of the file and the key.
    pub fn load(path: &Path) -> Result<Self, ConfigurationError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(ConfigurationError::read(path, error)),
        };
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| ConfigurationError::syntax(path, &error))?;
        let file = Self {
            path: path.to_owned(),
            document,
        };
        file.hosts()?;
        file.default_restore_policy()?;
        file.ended_session_retention()?;
        Ok(file)
    }

    /// The path of the file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The hosts of the file, in the sequence of their names.
    ///
    /// # Errors
    ///
    /// Returns an error with the key that is not valid.
    pub fn hosts(&self) -> Result<Vec<HostEntry>, ConfigurationError> {
        let Some(hosts) = self.document.get(HOSTS) else {
            return Ok(Vec::new());
        };
        let hosts = hosts
            .as_table_like()
            .ok_or_else(|| self.invalid(HOSTS, "must be a table"))?;
        let mut entries = Vec::new();
        for (name, host) in hosts.iter() {
            entries.push(self.host_entry(name, host)?);
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    /// The host `name`, if the file has it.
    ///
    /// # Errors
    ///
    /// Returns an error with the key that is not valid.
    pub fn host(&self, name: &str) -> Result<Option<HostEntry>, ConfigurationError> {
        Ok(self.hosts()?.into_iter().find(|host| host.name == name))
    }

    /// The restore policy of a session that has none: `default_policy` in
    /// `[restore]`. Returns `None` if the file does not have the key.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not `auto`, `on-focus` or `manual`.
    pub fn default_restore_policy(&self) -> Result<Option<RestorePolicy>, ConfigurationError> {
        self.text_value(RESTORE, DEFAULT_POLICY, |text| {
            text.parse()
                .map_err(|error: UnknownRestorePolicy| error.to_string())
        })
    }

    /// How long the daemon keeps a session whose command ended:
    /// `ended_retention` in `[sessions]`, for example `"24h"`. Returns
    /// `None` if the file does not have the key.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a number with the unit `s`,
    /// `m`, `h` or `d`.
    pub fn ended_session_retention(&self) -> Result<Option<Duration>, ConfigurationError> {
        self.text_value(SESSIONS, ENDED_RETENTION, |text| {
            duration_text::parse(text).ok_or_else(|| {
                "must be a number with the unit s, m, h or d, for example \"24h\"".to_owned()
            })
        })
    }

    /// The value of the string `key` of `table`, as `read` gives it.
    fn text_value<T>(
        &self,
        table: &str,
        key: &str,
        read: impl FnOnce(&str) -> Result<T, String>,
    ) -> Result<Option<T>, ConfigurationError> {
        let Some(item) = self.document.get(table) else {
            return Ok(None);
        };
        let Some(item) = item
            .as_table_like()
            .ok_or_else(|| self.invalid(table, "must be a table"))?
            .get(key)
        else {
            return Ok(None);
        };
        let full_key = format!("{table}.{key}");
        let text = item
            .as_str()
            .ok_or_else(|| self.invalid(&full_key, "must be a string"))?;
        read(text)
            .map(Some)
            .map_err(|problem| self.invalid(&full_key, &problem))
    }

    /// Adds the host `entry`, or replaces the host with the same name.
    pub fn set_host(&mut self, entry: &HostEntry) {
        let hosts = self.document.entry(HOSTS).or_insert_with(|| {
            let mut hosts = Table::new();
            // No `[hosts]` line: the file has only `[hosts.<name>]`.
            hosts.set_implicit(true);
            Item::Table(hosts)
        });
        hosts[entry.name.as_str()] = Item::Table(Table::new());
        hosts[entry.name.as_str()][SSH] = value(entry.ssh.as_str());
    }

    /// Removes the host `name`. Returns `false` if the file does not have
    /// it.
    pub fn remove_host(&mut self, name: &str) -> bool {
        self.document
            .get_mut(HOSTS)
            .and_then(Item::as_table_like_mut)
            .is_some_and(|hosts| hosts.remove(name).is_some())
    }

    /// Writes the file. The new content replaces the file in one step, and
    /// only the user can read it.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory or the file cannot be written.
    pub fn save(&self) -> Result<(), ConfigurationError> {
        self.write()
            .map_err(|error| ConfigurationError::write(&self.path, error))
    }

    fn write(&self) -> io::Result<()> {
        let directory = self.path.parent().unwrap_or(Path::new("."));
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY_DIRECTORY)
            .create(directory)?;
        let temporary = self.path.with_extension("toml.new");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(OWNER_READ_WRITE)
            .open(&temporary)?;
        file.write_all(self.document.to_string().as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &self.path)
    }

    fn host_entry(&self, name: &str, host: &Item) -> Result<HostEntry, ConfigurationError> {
        let key = format!("{HOSTS}.{name}");
        if !is_valid_host_name(name) {
            return Err(self.invalid(
                &key,
                "a host name has only the characters a to z, A to Z, 0 to 9, '.', '_' and '-'",
            ));
        }
        let host = host
            .as_table_like()
            .ok_or_else(|| self.invalid(&key, "must be a table"))?;
        let mut ssh = None;
        for (field, item) in host.iter() {
            let field_key = format!("{key}.{field}");
            if field != SSH {
                return Err(self.invalid(&field_key, "is not a known key"));
            }
            let target = item
                .as_str()
                .filter(|target| !target.trim().is_empty())
                .ok_or_else(|| self.invalid(&field_key, "must be a string that is not empty"))?;
            ssh = Some(target.to_owned());
        }
        let ssh = ssh.ok_or_else(|| self.invalid(&format!("{key}.{SSH}"), "is necessary"))?;
        Ok(HostEntry {
            name: name.to_owned(),
            ssh,
        })
    }

    fn invalid(&self, key: &str, problem: &str) -> ConfigurationError {
        ConfigurationError::invalid_key(&self.path, key, problem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(text: &str) -> Result<ConfigurationFile, ConfigurationError> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, text).unwrap();
        ConfigurationFile::load(&path)
    }

    #[test]
    fn an_error_names_the_file_and_the_key_that_is_not_valid() {
        let unknown_key = load("[hosts.vps]\nssh = \"a@b\"\nshh = \"typo\"\n").unwrap_err();
        let message = unknown_key.to_string();
        assert!(message.contains("config.toml"), "{message}");
        assert!(message.contains("hosts.vps.shh"), "{message}");

        let wrong_type = load("[hosts.vps]\nssh = 22\n").unwrap_err().to_string();
        assert!(wrong_type.contains("hosts.vps.ssh"), "{wrong_type}");

        let no_target = load("[hosts.vps]\n").unwrap_err().to_string();
        assert!(no_target.contains("hosts.vps.ssh"), "{no_target}");

        let bad_name = load("[hosts.\"a b\"]\nssh = \"a@b\"\n")
            .unwrap_err()
            .to_string();
        assert!(bad_name.contains("hosts.a b"), "{bad_name}");
    }

    #[test]
    fn the_session_settings_are_read_and_a_wrong_value_names_its_key() {
        let file =
            load("[restore]\ndefault_policy = \"manual\"\n[sessions]\nended_retention = \"2h\"\n")
                .unwrap();
        assert_eq!(
            file.default_restore_policy().unwrap(),
            Some(RestorePolicy::Manual)
        );
        assert_eq!(
            file.ended_session_retention().unwrap(),
            Some(Duration::from_hours(2))
        );

        // A key of a table that this module does not read is not an error.
        let other_keys = load("[restore]\nrevert = true\n[sessions]\n").unwrap();
        assert_eq!(other_keys.default_restore_policy().unwrap(), None);
        assert_eq!(other_keys.ended_session_retention().unwrap(), None);

        let policy = load("[restore]\ndefault_policy = \"always\"\n").unwrap_err();
        assert!(
            policy.to_string().contains("restore.default_policy"),
            "{policy}"
        );
        let retention = load("[sessions]\nended_retention = 24\n").unwrap_err();
        assert!(
            retention.to_string().contains("sessions.ended_retention"),
            "{retention}"
        );
    }

    #[test]
    fn a_change_of_the_hosts_keeps_the_other_parts_of_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nomux").join("config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let before = "# my settings\n[ui]\ntheme = \"nomux-dark\" # dark\n\n[hosts.old]\nssh = \"old.example.com\"\n";
        fs::write(&path, before).unwrap();

        let mut file = ConfigurationFile::load(&path).unwrap();
        file.set_host(&HostEntry {
            name: "vps.example.com".to_owned(),
            ssh: "deploy@vps.example.com".to_owned(),
        });
        assert!(file.remove_host("old"));
        assert!(!file.remove_host("old"));
        file.save().unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.starts_with("# my settings\n[ui]\ntheme = \"nomux-dark\" # dark\n"),
            "{after}"
        );
        assert!(!after.contains("old.example.com"), "{after}");
        let hosts = ConfigurationFile::load(&path).unwrap().hosts().unwrap();
        assert_eq!(
            hosts,
            [HostEntry {
                name: "vps.example.com".to_owned(),
                ssh: "deploy@vps.example.com".to_owned(),
            }]
        );
    }

    #[test]
    fn a_file_that_does_not_exist_is_an_empty_configuration_and_save_makes_it_private() {
        use std::os::unix::fs::MetadataExt;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nomux").join("config.toml");
        let mut file = ConfigurationFile::load(&path).unwrap();
        assert_eq!(file.hosts().unwrap(), []);

        file.set_host(&HostEntry {
            name: "vps".to_owned(),
            ssh: "vps".to_owned(),
        });
        file.save().unwrap();

        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[hosts.vps]\nssh = \"vps\"\n"
        );
    }
}
