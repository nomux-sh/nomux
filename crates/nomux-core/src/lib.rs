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
//! This crate contains the `--version` output that the three binaries share,
//! the lookup of a binary of the same installation, and the parts of the
//! global configuration file that the core uses ([`configuration`]): the
//! hosts, the default restore policy and the retention time of an ended
//! session. The glossary types, the error type and the other parts of the
//! configuration model are not written yet.

mod companion_binary;
pub mod configuration;
mod version;

pub use companion_binary::companion_binary;
pub use version::version_only_main;
