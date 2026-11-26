//! The status of the Agent run of each session.
//!
//! The `nomux-status` crate has the rules. This module collects what the
//! sources report, keeps the result for each session and tells the event
//! log each change. `docs/status.md` has the model.
//!
//! - `registry`: the status of each session, outside the session registry.
//! - `watcher`: one task for each session with a holder. It waits on the
//!   holder for a change of the program reports.
//! - `holder_sources`: the program reports of a holder as source reports.
//! - `foreground`: the foreground program of the terminal as a source
//!   report.
//! - `self_report`: what a program in a session can report with a request.
//! - `adapter_report`: what an adapter extension reports with a host
//!   service.
//! - `token_check`: why a request did not prove that it comes from a
//!   session.
//! - `report_text`: the rule for a reported text.
//! - `message`: the form of a status in the control protocol.

mod adapter_report;
mod foreground;
mod holder_sources;
mod message;
mod registry;
mod report_text;
mod self_report;
mod token_check;
mod watcher;

pub(crate) use adapter_report::{AdapterReportRefused, ChildReport, RunReport};
pub(crate) use message::reported_state;
pub(crate) use registry::StatusRegistry;
pub(crate) use self_report::SelfReport;
pub(crate) use token_check::TokenRefused;
pub(crate) use watcher::watch;
