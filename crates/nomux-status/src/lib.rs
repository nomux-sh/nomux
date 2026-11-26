//! Agent status engine of nomux.
//!
//! # Responsibility
//!
//! This crate calculates the status of the Agent run of a session and of its
//! Child work from what the status sources report.
//!
//! - [`State`] and [`NeedsYouKind`]: what an Agent run or a Child work does.
//! - [`Source`] and [`Sourced`]: where a part of a status comes from. The
//!   sources decide in a fixed sequence, and each part of a status keeps its
//!   source.
//! - [`SourceReport`], [`Observation`] and [`ChildObservation`]: what one
//!   source says at one moment. They are the input of the resolver.
//! - [`resolve`]: merges the reports of the sources into an [`AgentRun`].
//! - [`AgentRun`], [`ChildWork`] and [`Status`]: the result. An Agent run is
//!   a tree with [`MAX_DEPTH`] levels of Child work at most.
//! - [`Urgency`] and [`most_urgent`]: the aggregation rules.
//! - [`carried_over`]: the rule for a session that lost its process.
//! - [`AdapterSource`]: what an adapter extension reported, with the rules
//!   for a soft working report and for the tree of its Child work.
//!
//! `docs/status.md` has each rule with its reason.
//!
//! # Boundaries
//!
//! - This crate is pure. It does no I/O, reads no clock and has no async
//!   runtime, thus a test needs no process, socket or time.
//! - This crate does not collect what the sources report. The daemon
//!   collects it and gives it to this crate.
//! - This crate depends on no other nomux crate.

mod adapter_source;
mod agent_run;
mod aggregation;
mod observation;
mod resolver;
mod session_restore;
mod source;
mod state;
mod status;

pub use adapter_source::{AdapterRefusal, AdapterSource, ChildChange};
pub use agent_run::{AgentRun, ChildWork, MAX_CHILD_WORK, MAX_DEPTH};
pub use aggregation::{Urgency, most_urgent};
pub use observation::{ChildObservation, Observation, SourceReport};
pub use resolver::resolve;
pub use session_restore::carried_over;
pub use source::{Source, Sourced};
pub use state::{NeedsYouKind, State};
pub use status::{Status, Timestamp};
