//! Transport layer of nomux.
//!
//! # Responsibility
//!
//! This crate moves protocol frames between a client and the daemon. It
//! contains the Unix socket transport, the SSH channels, the QUIC streams and
//! the credit windows for flow control. Each traffic class (control, input,
//! terminal data, bulk data) has its own channel, thus bulk traffic cannot
//! delay input.
//!
//! # Boundaries
//!
//! - This crate does not define the protocol messages. The `proto` repository
//!   owns the schema and the frame codec.
//! - This crate does not interpret the frames that it moves.
//! - The holder does not use this crate. The holder has no async runtime.
//!
//! # Status
//!
//! This crate has no code yet. The Unix socket transport comes first, then
//! SSH, then QUIC.
