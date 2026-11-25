//! What all the extensions of one host share.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::broadcast;

use crate::config::HostConfig;
use crate::extension_event::ExtensionEvent;
use crate::host_services::HostServices;
use crate::registry::Registry;

pub(crate) struct HostContext {
    pub(crate) config: HostConfig,
    registry: Mutex<Registry>,
    /// Gives the events of all the extensions to the subscribers.
    pub(crate) events: broadcast::Sender<ExtensionEvent>,
    pub(crate) services: Arc<dyn HostServices>,
    /// `true` after the shutdown of the host. The host starts no process
    /// then.
    pub(crate) closed: AtomicBool,
}

impl HostContext {
    pub(crate) fn new(
        config: HostConfig,
        registry: Registry,
        services: Arc<dyn HostServices>,
    ) -> Self {
        // A channel without room is not valid.
        let (events, _) = broadcast::channel(config.limits.event_buffer.max(1));
        Self {
            config,
            registry: Mutex::new(registry),
            events,
            services,
            closed: AtomicBool::new(false),
        }
    }

    /// The installed extensions. Do not hold the result across an `await`.
    pub(crate) fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
