//! Shared types of the nomux core.
//!
//! # Responsibility
//!
//! This crate contains the types that more than one nomux crate uses: the
//! glossary types (host, project, session, agent), the error type and the
//! configuration model.
//!
//! # Boundaries
//!
//! - This crate does no I/O on a socket or a PTY.
//! - This crate has no async runtime. The holder uses this crate, and the
//!   holder has no runtime.
//! - This crate does not depend on a different nomux crate.
//!
//! # Status
//!
//! This crate contains only the `--version` output that the three binaries
//! share. The glossary types, the error type and the configuration model are
//! not written yet.

mod version;

pub use version::version_only_main;
