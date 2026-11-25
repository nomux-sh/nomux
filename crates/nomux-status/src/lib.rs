//! Agent status engine of nomux.
//!
//! # Responsibility
//!
//! This crate calculates the status of an agent from its status sources
//! (hooks, ACP, OSC sequences, screen content). It contains the resolver, the
//! rules for each agent tool and the aggregation of status across sessions.
//!
//! # Boundaries
//!
//! - This crate is pure. It does no I/O and has no async runtime, thus a test
//!   needs no process, socket or clock.
//! - This crate does not collect the status events. The daemon collects them
//!   and gives them to this crate.
//!
//! # Status
//!
//! This crate has no code yet.
