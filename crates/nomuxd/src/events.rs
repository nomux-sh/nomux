//! The events of the host: a log in memory and the subscriptions of the
//! clients. `spec/control.md` of the `proto` repository has the rules of
//! the Events service.
//!
//! - `log`: the last events, with a limit of their number and of their
//!   bytes.
//! - `subscription`: one client connection that gets the events from its
//!   position on, or a snapshot if the log does not have that position.

mod log;
mod subscription;

pub(crate) use log::EventLog;
#[cfg(test)]
pub(crate) use log::Read;
pub(crate) use subscription::Subscription;
