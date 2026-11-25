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
//! Milestone M0 contains only [`BuildInfo`] and the entry point that the
//! three binaries share. Milestone M1 adds the glossary types, the error type
//! and the configuration model.

mod build_info;

pub use build_info::{BuildInfo, version_only_main};
