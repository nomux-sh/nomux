//! Protocol client of nomux.
//!
//! # Responsibility
//!
//! This crate is the client side of the nomux protocol. The CLI, the desktop
//! client and the mobile clients use it to connect to a daemon, to do the
//! handshake and to send requests.
//!
//! - [`Host`]: the local host, or a remote host through SSH. It opens the
//!   connections to the daemon of the host and starts the daemon. For a
//!   remote host it also installs the nomux binaries: [`BinarySource`],
//!   [`Installation`] and [`BootstrapStep`].
//! - [`Client`]: one connection to a daemon, with one method for each
//!   request.
//! - [`Host::attach`]: the terminal of a session. [`TerminalOutput`] gives
//!   the screen and the output as [`TerminalEvent`] values, and
//!   [`TerminalInput`] sends the input. The attach connects again when its
//!   connection ends.
//! - [`check_host`]: the checks of a remote host, as [`HostCheck`] values.
//! - [`Extension`], [`ExtensionPackage`] and [`ExtensionPermissions`]: the
//!   extensions of a host. [`Client::install_extension`] installs one in two
//!   steps, and [`Client::call_extension`] calls a method of one.
//! - [`Client::agent_tree`], [`Client::explain_agent`] and
//!   [`Client::set_agent_status`]: the status of the Agent run of each
//!   session ([`AgentTree`], [`AgentRun`], [`AgentStatus`]) and where it
//!   comes from ([`StatusSource`]).
//! - [`Client::subscribe_events`]: the events of a host as an
//!   [`EventStream`] of [`HostEvent`] values.
//! - [`Client::send_hook_event`], [`Client::hook_install_plan`] and
//!   [`Client::hook_adapters`]: the hooks of the agent tools
//!   ([`HookEvent`], [`HookInstallPlan`], [`HookAdapter`]).
//! - [`plan_hook_install`], [`plan_hook_uninstall`] and [`hooks_of_tool`]:
//!   the hooks of a tool in its settings file on this computer, as a
//!   [`SettingsChange`] with a diff that writes only when it is applied.
//! - [`Client::workspace`] and the requests after it: the workspace tree of
//!   the Groups and the Projects of a host ([`Workspace`], [`WorkspaceTree`],
//!   [`Node`]). [`Workspace::apply`] keeps a copy current with the
//!   [`WorkspaceChange`] of each event, and [`WorkspaceTree::project_of`]
//!   gives the Project of a session. [`most_urgent`] gives the status of a
//!   Group from the statuses of its sessions.
//! - [`resolve_session`] and [`Client::find_session`]: the session that a
//!   target names: its identifier, its name, or a unique prefix of one of
//!   them. [`Client::open_session`] starts a session with a name or gives
//!   the session that has it, and [`Client::rename_session`] renames one.
//! - [`ClientError`]: why a connection or a request failed.
//! - [`Client::capture`] and [`Client::capture_lines`]: the scrollback of a
//!   session, without an attach.
//! - [`Client::session_memory`]: the memory that the processes of each
//!   session use, as [`SessionMemory`] values.
//! - [`Client::worktrees`]: the worktrees of the Projects, as
//!   [`ProjectWorktrees`] values. [`Client::plan_clean`] and
//!   [`Client::clean_worktrees`]: the plan of a clean and its run;
//!   [`Client::mark_worktree`], [`Client::trash_entries`],
//!   [`Client::restore_worktree`] and [`Client::empty_trash`]: the marks
//!   and the trash place.
//! - [`ClientKind`], [`Session`], [`SessionState`], [`SessionExit`],
//!   [`RestorePolicy`], [`NewSession`], [`WindowSize`] and [`HostInfo`]: the
//!   data of the requests and the responses.
//!
//! # Boundaries
//!
//! - This crate has no user interface and reads no command line. It does
//!   not read a keyboard and does not write to a terminal.
//! - This crate does not talk to a holder. Only the daemon talks to a holder.
//! - A caller needs no type of the `nomux-proto` crate.
//!
//! # Status
//!
//! The crate has the control requests for sessions, for extensions and for
//! the daemon, the attach to a session with the output of its journal, the
//! scrollback, the restore of a dormant session, and the two kinds of host:
//! the local host and a remote host through SSH, the status of the
//! sessions, the events of a host and its workspace tree.

mod agent;
mod agent_requests;
mod bootstrap;
mod client;
mod client_kind;
mod doctor;
mod error;
mod events;
mod extension;
mod extension_archive;
mod extension_requests;
mod hibernation;
mod hook_install;
mod hook_requests;
mod hooks;
mod host;
mod host_info;
mod local_host;
mod reconnect;
mod scrollback;
mod session;
mod session_memory;
mod session_target;
mod ssh_host;
mod terminal;
mod window_size;
mod workspace;
mod workspace_requests;
mod worktree;
mod worktree_clean;

pub use agent::{
    AgentDetails, AgentExplanation, AgentRun, AgentState, AgentStatus, AgentTree, ChildWork,
    NeedsYouKind, ReportedState, SourceExplanation, Sourced, StatusSource, most_urgent,
};
pub use bootstrap::{BinarySource, BootstrapStep, Installation};
pub use client::Client;
pub use client_kind::ClientKind;
pub use doctor::{HostCheck, check_host, check_local_host};
pub use error::ClientError;
pub use events::{EventBody, EventStream, EventType, HibernatedEvent, HostEvent};
pub use extension::{
    Extension, ExtensionCallError, ExtensionErrorCode, ExtensionPackage, ExtensionPermissions,
    ExtensionState,
};
pub use extension_archive::{
    ArchiveSource, ChecksumCheck, ExtensionArchive, fetch_extension_archive,
};
pub use hibernation::{Hibernation, HibernationReason};
pub use hook_install::{
    HooksState, SettingsChange, ToolHooks, hooks_of_tool, plan_hook_install, plan_hook_uninstall,
    tools_with_installed_hooks,
};
pub use hooks::{
    HookAdapter, HookEvent, HookInstallPlan, HookPlatform, SettingsEntry, SettingsPath,
};
pub use host::{Host, StagedPackage};
pub use host_info::{HostInfo, MemoryLimitSupport};
pub use nomux_core::configuration::RestorePolicy;
pub use nomux_core::workspace_tree::{Node, NodeKind, TreeError, WorkspaceTree};
pub use nomux_term::QueryFilter;
pub use scrollback::{CaptureRange, CapturedOutput};
pub use session::{ExitStatus, NewSession, Session, SessionExit, SessionState};
pub use session_memory::{MemoryMeasure, SessionMemory};
pub use session_target::{most_recent_session, resolve_session};
pub use terminal::{
    AttachEnd, AttachOptions, OutputPosition, TerminalEvent, TerminalInput, TerminalOutput,
};
pub use window_size::WindowSize;
pub use workspace::{Applied, Workspace, WorkspaceChange};
pub use worktree::{
    BuildOutput, Mark, MergedBy, ProjectWorktrees, Refusal, Worktree, WorktreeFlag, WorktreeSize,
    WorktreeState,
};
pub use worktree_clean::{
    CleanAction, CleanOutcome, CleanRequest, CleanResult, Cleaned, Plan, PlanItem, PlanStep,
    TrashEntry, TrashRemoval,
};
