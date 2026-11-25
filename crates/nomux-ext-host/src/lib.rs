//! Extension host of the nomux daemon.
//!
//! # Responsibility
//!
//! This crate starts the daemon part of each extension as a separate process,
//! only when a client needs it. It speaks framed messages with the extension
//! through standard input and standard output, and it applies the permissions
//! of the extension.
//!
//! # Boundaries
//!
//! - This crate contains no extension. Official extensions use the same API
//!   as all other extensions and are in a different repository.
//! - This crate does not define the extension API. The `extension-api`
//!   repository owns it.
//! - Only the daemon uses this crate.
//!
//! # Status
//!
//! Milestone M0 contains no code. Milestone M3 adds the extension host.
