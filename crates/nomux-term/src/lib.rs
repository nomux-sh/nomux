//! Terminal state for nomux.
//!
//! # Responsibility
//!
//! This crate is a thin abstraction over one VT library. It gives the holder
//! and the clients one interface for the parser, the screen state and the
//! screen snapshot. The abstraction lets the project replace the VT library.
//!
//! # Boundaries
//!
//! - This crate does not open a PTY and does no I/O. It changes bytes into
//!   terminal state.
//! - This crate has no async runtime. The holder uses this crate.
//! - No other nomux crate uses the VT library directly.
//!
//! # Status
//!
//! This crate has no code yet. A benchmark (speed, memory, correctness)
//! selects the VT library before the abstraction is written.
