//! The extension host, as the daemon uses it.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_extension::Permissions;
use nomux_extension::manifest::Manifest;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinSet;

use crate::call_error::CallError;
use crate::config::HostConfig;
use crate::extension_event::ExtensionEvent;
use crate::host_context::HostContext;
use crate::host_services::HostServices;
use crate::registry::{self, Enablement, InstallError, InstalledExtension, Registry, StateError};
use crate::supervisor::{RunState, Supervisor};

/// One installed extension and the state of its process.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtensionStatus {
    /// The manifest, the approved permissions and the enabled state.
    pub extension: InstalledExtension,
    /// What the process of the extension does now.
    pub run_state: RunState,
}

/// Runs the daemon parts of the installed extensions.
///
/// A host that no call uses has no process, no task and no timer. A clone
/// is a second handle to the same host.
#[derive(Clone)]
pub struct ExtensionHost {
    context: Arc<HostContext>,
    /// The extensions that got a call. An extension without a call has no
    /// supervisor.
    supervisors: Arc<Mutex<HashMap<String, Arc<Supervisor>>>>,
}

impl ExtensionHost {
    /// Reads the installed extensions of the directory in `config`. Starts
    /// no process. `services` answers the calls of the extensions.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory of the host cannot be read. A
    /// package that is not valid is not an error: the host does not use it.
    pub fn open(config: HostConfig, services: Arc<dyn HostServices>) -> io::Result<Self> {
        let registry = Registry::load(config.directory.clone())?;
        Ok(Self {
            context: Arc::new(HostContext::new(config, registry, services)),
            supervisors: Arc::default(),
        })
    }

    /// Reads the manifest of the package in the directory `source` and
    /// examines if the host can run the package. Installs nothing. The
    /// manifest has the permissions that the extension asks for: show them
    /// to the user before [`install`](Self::install).
    ///
    /// This function reads files and blocks its thread.
    ///
    /// # Errors
    ///
    /// Returns an error if the package is not valid or if the host cannot
    /// run it.
    pub fn inspect(source: &Path) -> Result<Manifest, InstallError> {
        registry::inspect(source)
    }

    /// Installs the package in the directory `source` and enables it.
    /// `granted` has the permissions that the user approved. Starts no
    /// process.
    ///
    /// This function copies files and blocks its thread. During the copy,
    /// the other functions of the host do not wait.
    ///
    /// # Errors
    ///
    /// Returns an error if the package is not valid, if the host cannot run
    /// it, if `granted` has a permission that the manifest does not ask
    /// for, or if the extension is already installed.
    pub fn install(
        &self,
        source: &Path,
        granted: Permissions,
    ) -> Result<InstalledExtension, InstallError> {
        let pending = self.context.registry().begin_install(source, granted)?;
        let copied = pending.copy_package();
        self.context.registry().finish_install(pending, copied)
    }

    /// Stops the process of an extension and removes its package, its state
    /// file and its data directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not installed or if a file was
    /// not removed. The extension is not installed after this function also
    /// when a file of it stays.
    pub async fn remove(&self, id: &str) -> Result<(), StateError> {
        // From this moment no call starts the extension.
        let removed = self.context.registry().remove(id)?;
        let supervisor = self.supervisors().remove(id);
        if let Some(supervisor) = supervisor {
            supervisor.stop().await;
        }
        let deleted = tokio::task::spawn_blocking(move || removed.delete_files()).await;
        deleted
            .unwrap_or_else(|error| Err(io::Error::other(error)))
            .map_err(StateError::Io)
    }

    /// The installed extensions, in the sequence of their identifiers.
    #[must_use]
    pub fn extensions(&self) -> Vec<ExtensionStatus> {
        let registry = self.context.registry();
        let statuses = registry.extensions().map(|extension| ExtensionStatus {
            extension: extension.clone(),
            run_state: self.run_state_of(&extension.manifest.id),
        });
        statuses.collect()
    }

    /// One installed extension. `None` if it is not installed.
    #[must_use]
    pub fn extension(&self, id: &str) -> Option<ExtensionStatus> {
        let extension = self.context.registry().get(id)?.clone();
        Some(ExtensionStatus {
            extension,
            run_state: self.run_state_of(id),
        })
    }

    /// The state of the process of an extension, and each change of it.
    /// `None` if the extension is not installed.
    #[must_use]
    pub fn watch_run_state(&self, id: &str) -> Option<watch::Receiver<RunState>> {
        Some(self.supervisor(id)?.run_state())
    }

    /// The last bytes that the extension wrote to its standard error. The
    /// host keeps `Limits::stderr_log_bytes` bytes for each extension.
    #[must_use]
    pub fn stderr_log(&self, id: &str) -> Vec<u8> {
        self.started_supervisor(id)
            .map_or_default(|supervisor| supervisor.stderr())
    }

    /// Calls a method of an extension and waits for the result. The first
    /// call starts the process of the extension.
    ///
    /// To cancel the call, drop the result of this function before it
    /// completes: the host tells the extension to stop the work.
    ///
    /// # Errors
    ///
    /// Returns an error if the host cannot start the extension, if the
    /// process fails or does not answer in `Limits::call`, or if the
    /// extension answers with an error.
    pub async fn call(
        &self,
        id: &str,
        method: &str,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, CallError> {
        let supervisor = self.supervisor(id).ok_or(CallError::NotInstalled)?;
        supervisor.call(method, payload).await
    }

    /// The events of all the extensions from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ExtensionEvent> {
        self.context.events.subscribe()
    }

    /// Enables an extension, also after the host disabled it because of
    /// failures.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not installed or if the state
    /// file was not written.
    pub async fn enable(&self, id: &str) -> Result<(), StateError> {
        let enabled = self
            .context
            .registry()
            .set_enablement(id, Enablement::Enabled);
        if let Some(supervisor) = self.started_supervisor(id) {
            supervisor.forget_failures().await;
        }
        enabled
    }

    /// Disables an extension and stops its process. `reason` says why, for
    /// the user.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not installed or if the state
    /// file was not written. The extension is disabled also when the file
    /// was not written.
    pub async fn disable(&self, id: &str, reason: impl Into<String>) -> Result<(), StateError> {
        let enablement = Enablement::Disabled {
            reason: reason.into(),
        };
        let disabled = self.context.registry().set_enablement(id, enablement);
        if let Some(supervisor) = self.started_supervisor(id) {
            supervisor.stop().await;
        }
        disabled
    }

    /// Stops each process and waits for its end. The daemon calls it
    /// before it stops. After it, the host starts no extension: a call
    /// fails with [`CallError::Stopped`].
    pub async fn shutdown(&self) {
        self.context.closed.store(true, Ordering::Relaxed);
        let supervisors: Vec<Arc<Supervisor>> = {
            let supervisors = self.supervisors();
            supervisors.values().cloned().collect()
        };
        let mut stops = JoinSet::new();
        for supervisor in supervisors {
            stops.spawn(async move { supervisor.stop().await });
        }
        while stops.join_next().await.is_some() {}
    }

    fn run_state_of(&self, id: &str) -> RunState {
        self.started_supervisor(id)
            .map_or(RunState::Stopped, |supervisor| {
                supervisor.run_state().borrow().clone()
            })
    }

    /// The supervisor of an installed extension. The first use makes it.
    fn supervisor(&self, id: &str) -> Option<Arc<Supervisor>> {
        let mut supervisors = self.supervisors();
        if let Some(supervisor) = supervisors.get(id) {
            return Some(Arc::clone(supervisor));
        }
        self.context.registry().get(id)?;
        let supervisor = Arc::new(Supervisor::new(id.to_owned(), Arc::clone(&self.context)));
        supervisors.insert(id.to_owned(), Arc::clone(&supervisor));
        Some(supervisor)
    }

    /// The supervisor of an extension that got a call before.
    fn started_supervisor(&self, id: &str) -> Option<Arc<Supervisor>> {
        self.supervisors().get(id).cloned()
    }

    fn supervisors(&self) -> MutexGuard<'_, HashMap<String, Arc<Supervisor>>> {
        self.supervisors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for ExtensionHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionHost")
            .field("config", &self.context.config)
            .finish_non_exhaustive()
    }
}
