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
//! the lookup of a binary of the same installation and of a program in
//! `PATH`, and the parts of the global configuration file that the core
//! uses ([`configuration`]): the hosts, the default restore policy, the
//! retention time of an ended session, the memory limits and the
//! hibernation settings. It also has the data directory of nomux ([`data_directory`])
//! and the rules of a hook event that the CLI and the
//! daemon share ([`hook_event`]), and the runtime directory with the rule
//! for the paths of the sockets ([`runtime_directory`]), and the workspace
//! tree of the Groups and the Projects of a host with its rules, which the
//! daemon and the clients share ([`workspace_tree`]). The glossary types,
//! the error type and the other parts of the configuration model are not
//! written yet.

mod companion_binary;
pub mod configuration;
mod data_directory;
pub mod hook_event;
mod program_path;
pub mod runtime_directory;
mod version;
pub mod workspace_tree;

pub use companion_binary::companion_binary;
pub use data_directory::data_directory;
pub use program_path::program_in_path;
pub use version::version_only_main;
