//! The hook events of the agent tools and the adapter extensions that take
//! them. `spec/adapter.md` of the `proto` repository has the rules.
//!
//! - `adapters`: which extension is the adapter of a tool, and the methods
//!   of an adapter.
//! - `delivery`: the queue of the hook events that an adapter did not take
//!   yet.

mod adapters;
mod delivery;

pub(crate) use adapters::{PLAN_METHOD, adapter_of, adapters};
pub(crate) use delivery::HookDelivery;
