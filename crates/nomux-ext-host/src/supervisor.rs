//! The life cycle of one extension across its processes: start on the
//! first call, start again after a failure, disable after too many
//! failures.

mod restart_policy;
mod run_state;

use std::env;
use std::sync::Arc;
use std::time::Instant;

use nomux_extension::ApiVersion;
use tokio::sync::{Mutex, OwnedMutexGuard, oneshot, watch};
use tokio::task::yield_now;
use tracing::{error, info, warn};

pub use run_state::RunState;

use crate::call_error::CallError;
use crate::connection::{
    Connection, ConnectionHandle, End, Identity, InFlightCall, Launch, Setup, StderrLog,
};
use crate::failure::Failure;
use crate::host_context::HostContext;
use crate::registry::Enablement;
use restart_policy::{RestartPolicy, Verdict};

/// The state that the callers and the task of the connection share. A
/// caller holds the lock while it starts the process or gives it a call.
#[derive(Default)]
struct Lifecycle {
    /// The process that runs. `None` if no process runs.
    connection: Option<ConnectionHandle>,
    restarts: RestartPolicy,
}

/// Starts and stops the process of one extension. Without a call, a
/// supervisor has no process, no task and no timer.
pub(crate) struct Supervisor {
    extension_id: String,
    context: Arc<HostContext>,
    lifecycle: Arc<Mutex<Lifecycle>>,
    state: watch::Sender<RunState>,
    stderr_log: Arc<StderrLog>,
}

impl Supervisor {
    pub(crate) fn new(extension_id: String, context: Arc<HostContext>) -> Self {
        let stderr_log = StderrLog::new(context.config.limits.stderr_log_bytes);
        Self {
            extension_id,
            context,
            lifecycle: Arc::default(),
            state: watch::Sender::new(RunState::Stopped),
            stderr_log: Arc::new(stderr_log),
        }
    }

    pub(crate) fn run_state(&self) -> watch::Receiver<RunState> {
        self.state.subscribe()
    }

    /// The last bytes of the standard error of the extension.
    pub(crate) fn stderr(&self) -> Vec<u8> {
        self.stderr_log.contents()
    }

    /// Calls a method of the extension. Starts the process if it does not
    /// run. If the caller drops the result before it completes, the host
    /// cancels the call.
    pub(crate) async fn call(
        self: &Arc<Self>,
        method: &str,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, CallError> {
        let call = self.begin_call(method, payload).await?;
        call.result(self.context.config.limits.call).await
    }

    async fn begin_call(
        self: &Arc<Self>,
        method: &str,
        payload: Vec<u8>,
    ) -> Result<InFlightCall, CallError> {
        loop {
            let lifecycle = Arc::clone(&self.lifecycle).lock_owned().await;
            match &lifecycle.connection {
                Some(connection) if !connection.is_closed() => {
                    return connection.begin_call(method, payload);
                }
                Some(_) => {
                    // The process ended at this moment. Its task waits for
                    // the lock to record the end.
                    drop(lifecycle);
                    yield_now().await;
                }
                None => self.start(lifecycle).await?,
            }
        }
    }

    /// Starts the process and waits until it serves calls.
    async fn start(
        self: &Arc<Self>,
        lifecycle: OwnedMutexGuard<Lifecycle>,
    ) -> Result<(), CallError> {
        let setup = self.prepare_start(&lifecycle)?;
        let (ready, started) = oneshot::channel();
        // The process has its own task: a caller that stops waiting does
        // not stop a start that other callers wait for.
        tokio::spawn(Arc::clone(self).run_connection(setup, lifecycle, ready));
        match started.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(failure)) => Err(CallError::Failed(failure)),
            Err(_) => Err(CallError::Stopped),
        }
    }

    /// Examines if the host can start the extension now.
    fn prepare_start(&self, lifecycle: &Lifecycle) -> Result<Setup, CallError> {
        let registry = self.context.registry();
        let extension = registry
            .get(&self.extension_id)
            .ok_or(CallError::NotInstalled)?;
        if let Enablement::Disabled { reason } = &extension.enablement {
            return Err(CallError::Disabled {
                reason: reason.clone(),
            });
        }
        let program = registry.program(extension).ok_or(CallError::NoDaemonPart)?;
        if !extension.manifest.api.matches(ApiVersion::CURRENT) {
            return Err(CallError::IncompatibleApi {
                required: extension.manifest.api,
                host: ApiVersion::CURRENT,
            });
        }
        let pause_left = lifecycle.restarts.pause_left(Instant::now());
        let state = self.state.borrow().clone();
        if let (Some(retry_in), RunState::Failed { failure, .. }) = (pause_left, state) {
            return Err(CallError::BackingOff { failure, retry_in });
        }

        let config = &self.context.config;
        let inherited = config.inherited_variables.iter();
        let environment = inherited.filter_map(|name| Some((name.clone(), env::var_os(name)?)));
        Ok(Setup {
            launch: Launch {
                program,
                working_directory: registry.data_directory(&self.extension_id),
                environment: environment.collect(),
            },
            identity: Identity {
                extension_id: self.extension_id.clone(),
                api: extension.manifest.api,
                granted: extension.granted.clone(),
            },
            limits: config.limits.clone(),
            services: Arc::clone(&self.context.services),
            events: self.context.events.clone(),
            stderr_log: Arc::clone(&self.stderr_log),
        })
    }

    /// The task of one process: starts it, serves it and records its end.
    /// The task has the lock from the start until the process serves
    /// calls, thus the callers wait for the start.
    async fn run_connection(
        self: Arc<Self>,
        setup: Setup,
        mut lifecycle: OwnedMutexGuard<Lifecycle>,
        ready: oneshot::Sender<Result<(), Failure>>,
    ) {
        self.state.send_replace(RunState::Starting);
        let connection = match Connection::open(setup, Arc::clone(&self.lifecycle)).await {
            Ok((handle, connection)) => {
                lifecycle.connection = Some(handle);
                self.state.send_replace(RunState::Running);
                drop(lifecycle);
                // A caller that is gone stopped waiting.
                let _ = ready.send(Ok(()));
                connection
            }
            Err(failure) => {
                self.record_end(&mut lifecycle, End::Failed(failure.clone()));
                drop(lifecycle);
                // A caller that is gone stopped waiting.
                let _ = ready.send(Err(failure));
                return;
            }
        };
        let (end, held) = connection.serve().await;
        let mut lifecycle = match held {
            Some(lifecycle) => lifecycle,
            None => Arc::clone(&self.lifecycle).lock_owned().await,
        };
        self.record_end(&mut lifecycle, end);
    }

    /// Records how a process ended and decides when the extension can
    /// start again.
    fn record_end(&self, lifecycle: &mut Lifecycle, end: End) {
        lifecycle.connection = None;
        let failure = match end {
            End::Stopped => {
                lifecycle.restarts.reset();
                self.state.send_replace(RunState::Stopped);
                return;
            }
            End::Failed(failure) => failure,
        };
        warn!(extension = %self.extension_id, %failure, "the extension failed");
        let verdict = if failure.is_permanent() {
            Verdict::GiveUp
        } else {
            let limits = &self.context.config.limits;
            lifecycle.restarts.record_failure(Instant::now(), limits)
        };
        let retry_at = match verdict {
            Verdict::RetryAt(retry_at) => Some(retry_at),
            Verdict::GiveUp => {
                self.disable(&failure);
                None
            }
        };
        self.state
            .send_replace(RunState::Failed { failure, retry_at });
    }

    fn disable(&self, failure: &Failure) {
        info!(extension = %self.extension_id, "the host disables the extension");
        let enablement = Enablement::Disabled {
            reason: format!("the extension failed: {failure}"),
        };
        let written = self
            .context
            .registry()
            .set_enablement(&self.extension_id, enablement);
        if let Err(error) = written {
            // The extension is disabled until the daemon stops.
            error!(extension = %self.extension_id, %error, "the disabled state was not saved");
        }
    }

    /// Stops the process, if one runs, and waits for its end.
    pub(crate) async fn stop(&self) {
        let stopped = {
            let lifecycle = self.lifecycle.lock().await;
            let Some(connection) = &lifecycle.connection else {
                return;
            };
            connection.stop()
        };
        // The channel closes when the task of the connection ends.
        let _ = stopped.await;
        // The task records the end with the lock. After this lock, the
        // state is the state after the stop.
        drop(self.lifecycle.lock().await);
    }

    /// Forgets the failures. The user enabled the extension again.
    pub(crate) async fn forget_failures(&self) {
        let mut lifecycle = self.lifecycle.lock().await;
        lifecycle.restarts.reset();
        if lifecycle.connection.is_none() {
            self.state.send_replace(RunState::Stopped);
        }
    }
}
