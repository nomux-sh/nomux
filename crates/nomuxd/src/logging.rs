//! The log of the daemon: `tracing` events on the standard error.

use std::env;
use std::io;

use tracing::Level;

/// The variable that sets the log level.
const LEVEL_VARIABLE: &str = "NOMUX_LOG";
/// Quiet by default: a daemon without a problem writes nothing.
const DEFAULT_LEVEL: Level = Level::WARN;

/// Starts the log. The level comes from the `NOMUX_LOG` variable.
pub(crate) fn start() {
    let requested = env::var(LEVEL_VARIABLE).ok();
    let level = requested.as_deref().map(str::parse::<Level>);
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_max_level(match level {
            Some(Ok(level)) => level,
            _ => DEFAULT_LEVEL,
        })
        .init();
    if let (Some(requested), Some(Err(_))) = (requested, level) {
        tracing::warn!(
            "{LEVEL_VARIABLE}={requested} is not a log level; the level is {DEFAULT_LEVEL}"
        );
    }
}
