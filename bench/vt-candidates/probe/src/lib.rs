//! The measurements that each VT library probe runs.
//!
//! A probe is one binary for one candidate. It implements [`Parser`] (and
//! [`Screen`], if the candidate keeps a screen) and calls [`run_parser`] or
//! [`run_screen`]. All probes share this crate, thus the difference between
//! two probe binaries is the candidate only.

mod candidate;
mod command;
mod conformance;
mod corpus;
mod memory;
mod throughput;

pub use candidate::{Parser, Position, Screen, Size};
pub use command::{run_parser, run_screen};
pub use corpus::Corpus;
