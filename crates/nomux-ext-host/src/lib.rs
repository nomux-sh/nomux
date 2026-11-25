//! Extension host of the nomux daemon.
//!
//! # Responsibility
//!
//! This crate is the host side of the extension protocol. It keeps the
//! installed extensions, starts the daemon part of an extension as a
//! separate process when a call needs it, and stops the process when it is
//! idle. It speaks framed messages with the process through its standard
//! input and its standard output, and it applies the permissions that the
//! user approved.
//!
//! A failure of an extension stays in its process: the host ends the
//! process, fails the calls that wait, and starts the extension again for a
//! later call. After too many failures, the host disables the extension.
//!
//! `docs/extensions.md` in the repository describes the model, the life
//! cycle and the limits.
//!
//! # Use
//!
//! The daemon makes one [`ExtensionHost`] with [`ExtensionHost::open`] and
//! gives it its [`HostServices`]. [`ExtensionHost::call`] calls a method of
//! an extension, and [`ExtensionHost::subscribe`] gives the events of the
//! extensions. The functions that start a process or wait need a Tokio
//! runtime.
//!
//! # Boundaries
//!
//! - This crate contains no extension. Official extensions use the same API
//!   as all other extensions and are in a different repository.
//! - This crate does not define the extension API. The `extension-api`
//!   repository owns it, and the crate `nomux-extension` has its messages,
//!   its framing and the manifest validator.
//! - This crate has no host service. The daemon supplies them through
//!   [`HostServices`].
//! - Only the daemon uses this crate.

mod call_error;
mod config;
mod connection;
mod extension_event;
mod failure;
mod host;
mod host_context;
mod host_services;
mod registry;
mod supervisor;

pub use call_error::CallError;
pub use config::{HostConfig, Limits};
pub use extension_event::ExtensionEvent;
pub use failure::Failure;
pub use host::{ExtensionHost, ExtensionStatus};
pub use host_services::{HostServices, ServiceError, ServiceReply};
pub use nomux_extension::manifest::Manifest;
pub use nomux_extension::{ErrorCode, Permissions};
pub use registry::{Enablement, InstallError, InstalledExtension, StateError};
pub use supervisor::RunState;
