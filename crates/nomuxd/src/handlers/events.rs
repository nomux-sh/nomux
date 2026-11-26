//! The handler of the Events service.

use std::sync::Arc;

use nomux_proto::wire::v1::EventsSubscribeRequest;

use super::Context;
use crate::events::Subscription;

pub(super) fn subscribe(request: &EventsSubscribeRequest, context: &Context) -> Subscription {
    Subscription::new(
        request,
        Arc::clone(&context.events),
        Arc::clone(&context.status),
        Arc::clone(&context.sessions),
    )
}
