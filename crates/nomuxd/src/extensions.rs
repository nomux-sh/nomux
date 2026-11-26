//! The extensions of the host: the extension host of the daemon, its
//! directory and the host services that the daemon supplies.
//!
//! The host starts no process and no task here. The first call of a method
//! of an extension starts the process of that extension.

mod agent_services;
mod data_directory;
mod host_services;

use std::env;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use nomux_ext_host::{ExtensionHost, HostConfig};

use crate::events::EventLog;
use crate::sessions::Sessions;
use crate::status::StatusRegistry;
use host_services::DaemonServices;

/// The variable with the idle time of an extension in seconds.
const IDLE_VARIABLE: &str = "NOMUX_EXTENSION_IDLE_SECONDS";

/// Makes the extension host of the daemon with the installed extensions of
/// the user. The extensions get the host services of the daemon, and their
/// events go to `events`.
pub(crate) fn open(
    sessions: Arc<Sessions>,
    status: Arc<StatusRegistry>,
    events: Arc<EventLog>,
) -> io::Result<ExtensionHost> {
    let directory = data_directory::locate()?;
    data_directory::prepare(&directory)?;
    let mut config = HostConfig::new(directory);
    if let Some(idle) = idle_time_of_environment()? {
        config.limits.idle = idle;
    }
    let services = DaemonServices::new(sessions, status, events);
    ExtensionHost::open(config, Arc::new(services))
}

/// The idle time that the environment of the daemon sets. `None` if the
/// variable is not set: the host uses its default.
fn idle_time_of_environment() -> io::Result<Option<Duration>> {
    let Some(value) = env::var_os(IDLE_VARIABLE) else {
        return Ok(None);
    };
    idle_time(&value.to_string_lossy())
        .map(Some)
        .ok_or_else(|| {
            io::Error::other(format!(
                "{IDLE_VARIABLE} must be a number of seconds from 1 to 86400, but it is `{}`",
                value.display()
            ))
        })
}

fn idle_time(seconds: &str) -> Option<Duration> {
    const ONE_DAY: u64 = 86400;
    let seconds: u64 = seconds.parse().ok()?;
    (1..=ONE_DAY)
        .contains(&seconds)
        .then(|| Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_time_is_a_number_of_seconds_from_one_second_to_one_day() {
        assert_eq!(idle_time("1"), Some(Duration::from_secs(1)));
        assert_eq!(idle_time("86400"), Some(Duration::from_hours(24)));
        // Without an idle time, a process stops immediately after its start.
        assert_eq!(idle_time("0"), None);
        assert_eq!(idle_time("86401"), None);
        assert_eq!(idle_time("1.5"), None);
        assert_eq!(idle_time(""), None);
    }
}
