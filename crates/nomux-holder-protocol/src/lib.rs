//! Contract between a holder and the daemon.
//!
//! # Responsibility
//!
//! This crate is the one definition of what a holder and the daemon share:
//!
//! - [`wire`]: the control messages on the Unix socket of a holder.
//! - [`ring`]: the layout of the ring buffer in shared memory, its writer
//!   (the holder) and its reader (the daemon).
//! - [`redraw`]: the shared memory that carries the redraw sequence of a
//!   screen from the holder to the daemon.
//! - [`discovery`]: the discovery files of a holder, and the scan that tells
//!   a live holder from a stale directory.
//!
//! A holder that runs is never replaced. Thus each item here is a contract
//! with all the holders that already run: a change must be an addition.
//! `docs/holder.md` has the rules.
//!
//! # Boundaries
//!
//! - This crate has no async runtime. The holder uses this crate.
//! - This crate depends on no other nomux crate.
//! - This crate does not open a PTY and does not start a process.

pub mod discovery;
pub mod redraw;
pub mod ring;
pub mod wire;

mod mapping;
mod session_id;

pub use session_id::{InvalidSessionId, SessionId};
