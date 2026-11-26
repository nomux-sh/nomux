//! The file that starts the daemon at login: a systemd user unit on Linux,
//! a launchd agent on macOS.
//!
//! The commands of this module write and remove files only in the
//! configuration directories of the user, and they start no program. The
//! user gives the file to the service manager: the report has the command.

mod launchd_agent;
mod systemd_unit;

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nomux_client::ClientError;

use crate::report::{Report, ServiceFileAction, ServiceFileReport};

const DAEMON_BINARY: &str = "nomuxd";

/// The service file of one service manager, for this user.
struct ServiceFile {
    path: PathBuf,
    content: String,
    /// The link that the service manager makes when the user enables the
    /// service. It has no target after the file is removed.
    enabled_link: Option<PathBuf>,
    /// The command that gives the file to the service manager.
    after_install: String,
    /// The command that makes the service manager forget the file.
    after_uninstall: String,
}

/// Writes the service file. With `dry_run`, only reports its path and its
/// content.
pub(crate) fn install(dry_run: bool) -> Result<Report, ClientError> {
    let service = ServiceFile::of_this_user()?;
    let action = if dry_run {
        ServiceFileAction::WouldInstall
    } else {
        write(&service.path, &service.content)
            .map_err(|error| cannot("write", &service, &error))?;
        ServiceFileAction::Installed
    };
    Ok(Report::ServiceFile(ServiceFileReport {
        action,
        path: service.path,
        content: Some(service.content),
        next_step: Some(service.after_install),
    }))
}

/// Removes the service file. With `dry_run`, only reports its path. The
/// daemon that runs is not stopped.
pub(crate) fn uninstall(dry_run: bool) -> Result<Report, ClientError> {
    let service = ServiceFile::of_this_user()?;
    let action = match (service.path.exists(), dry_run) {
        (false, _) => ServiceFileAction::NotInstalled,
        (true, true) => ServiceFileAction::WouldRemove,
        (true, false) => {
            remove(&service).map_err(|error| cannot("remove", &service, &error))?;
            ServiceFileAction::Removed
        }
    };
    let next_step = (action == ServiceFileAction::Removed).then_some(service.after_uninstall);
    Ok(Report::ServiceFile(ServiceFileReport {
        action,
        path: service.path,
        content: None,
        next_step,
    }))
}

impl ServiceFile {
    /// The service file of the operating system of this computer.
    fn of_this_user() -> Result<Self, ClientError> {
        let daemon = daemon_path()?;
        let home = env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| ClientError::Failed("the HOME variable is not set".to_owned()))?;
        let service = if cfg!(target_os = "macos") {
            let path = launchd_agent::path(&home);
            Self {
                content: launchd_agent::content(&daemon),
                enabled_link: None,
                after_install: format!("launchctl bootstrap gui/$(id -u) '{}'", path.display()),
                after_uninstall: format!("launchctl bootout gui/$(id -u)/{}", launchd_agent::LABEL),
                path,
            }
        } else {
            let directory = systemd_unit::directory(&home, env::var_os("XDG_CONFIG_HOME"));
            Self {
                content: systemd_unit::content(&daemon),
                enabled_link: Some(systemd_unit::enabled_link(&directory)),
                after_install: format!(
                    "systemctl --user daemon-reload && systemctl --user enable --now {}",
                    systemd_unit::UNIT
                ),
                after_uninstall: "systemctl --user daemon-reload".to_owned(),
                path: directory.join(systemd_unit::UNIT),
            }
        };
        Ok(service)
    }
}

/// The absolute path of the `nomuxd` of this installation. A service
/// manager does not look for a program in `PATH`.
fn daemon_path() -> Result<PathBuf, ClientError> {
    let daemon = nomux_core::companion_binary(DAEMON_BINARY);
    let is_usable = daemon.is_absolute() && daemon.to_str().is_some_and(is_one_line);
    if is_usable {
        Ok(daemon)
    } else {
        Err(ClientError::Failed(format!(
            "the service file needs the path of {DAEMON_BINARY}: {DAEMON_BINARY} must be in the \
             directory of nomux, and the path must be one line of text"
        )))
    }
}

fn is_one_line(text: &str) -> bool {
    !text.chars().any(char::is_control)
}

fn write(path: &Path, content: &str) -> io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    fs::write(path, content)
}

fn remove(service: &ServiceFile) -> io::Result<()> {
    fs::remove_file(&service.path)?;
    match service.enabled_link.as_deref().map(fs::remove_file) {
        Some(Err(error)) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn cannot(action: &str, service: &ServiceFile, error: &io::Error) -> ClientError {
    ClientError::Failed(format!(
        "cannot {action} the service file {}: {error}",
        service.path.display()
    ))
}
