//! Protocol client of nomux.
//!
//! # Responsibility
//!
//! This crate is the client side of the nomux protocol. The CLI, the desktop
//! client and the mobile clients use it to connect to a daemon, to do the
//! capability handshake, to send requests and to receive terminal data and
//! events.
//!
//! # Boundaries
//!
//! - This crate has no user interface and reads no command line.
//! - This crate does not talk to a holder. Only the daemon talks to a holder.
//!
//! # Status
//!
//! This crate has no code yet.
