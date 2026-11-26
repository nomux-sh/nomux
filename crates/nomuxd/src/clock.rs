//! The time of the daemon for the status of the sessions and for events.

use std::time::{SystemTime, UNIX_EPOCH};

use nomux_status::Timestamp;

/// Now: milliseconds after the Unix epoch. A clock before the epoch gives
/// 0.
pub(crate) fn now() -> Timestamp {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp(u64::try_from(since_epoch.as_millis()).unwrap_or(u64::MAX))
}
