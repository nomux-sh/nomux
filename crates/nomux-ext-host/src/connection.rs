//! One process of an extension, from its start to its end, and the task
//! that owns it.

mod connection_handle;
mod envelope_reader;
mod envelopes;
mod extension_process;
mod flood_meter;
mod handshake;
mod health_check;
mod launch;
mod pending_calls;
mod service_calls;
mod stderr_log;

use std::sync::Arc;

use nomux_extension::wire::v1::envelope::Kind;
use nomux_extension::wire::v1::request::Body;
use nomux_extension::wire::v1::{
    Envelope, Event, HealthRequest, InitializeRequest, Request, Response, ShutdownRequest, response,
};
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast, mpsc, oneshot};
use tokio::time::{Instant, sleep_until, timeout};

pub(crate) use connection_handle::{ConnectionHandle, InFlightCall};
pub(crate) use handshake::Identity;
pub(crate) use launch::Launch;
pub(crate) use stderr_log::StderrLog;

use crate::call_error::CallError;
use crate::config::Limits;
use crate::extension_event::ExtensionEvent;
use crate::failure::Failure;
use crate::host_services::{HostServices, ServiceError};
use connection_handle::{Command, RequestIds};
use extension_process::{ExtensionProcess, ProcessEvent};
use health_check::{HealthCheck, HealthStep};
use pending_calls::PendingCalls;
use service_calls::ServiceCalls;

/// What a connection needs to start the process of an extension.
pub(crate) struct Setup {
    pub(crate) launch: Launch,
    pub(crate) identity: Identity,
    pub(crate) limits: Limits,
    pub(crate) services: Arc<dyn HostServices>,
    pub(crate) events: broadcast::Sender<ExtensionEvent>,
    pub(crate) stderr_log: Arc<StderrLog>,
}

/// How a connection ended.
#[derive(Debug)]
pub(crate) enum End {
    /// The host stopped the extension, and the process ended as the
    /// protocol says.
    Stopped,
    /// The process failed, or the host ended it.
    Failed(Failure),
}

/// The step of the life cycle of the process.
#[derive(Clone, Copy)]
enum Phase {
    /// `Welcome` did not arrive yet.
    Greeting,
    /// `InitializeRequest` has no response yet.
    Initializing { request_id: u64, deadline: Instant },
    /// The two peers send calls.
    Serving,
    /// `ShutdownRequest` has no response yet.
    ShuttingDown { request_id: u64, deadline: Instant },
    /// The extension answered `ShutdownRequest`. The process must end.
    Exiting { deadline: Instant },
    /// The process ended after a shutdown.
    Ended,
}

/// What the task of a connection waited for.
enum Input {
    Process(Result<ProcessEvent, Failure>),
    Command(Command),
    ServiceResponse(Envelope),
    Deadline,
}

/// The process of one extension and the state of its protocol.
///
/// `Owner` is the state of the part of the host that gives calls to the
/// connection. A caller holds the lock of the owner while it gives a call.
/// The connection takes the same lock before it stops an idle process, thus
/// no call can arrive for a process that stops.
pub(crate) struct Connection<Owner> {
    process: ExtensionProcess,
    commands: mpsc::UnboundedReceiver<Command>,
    request_ids: RequestIds,
    extension_id: String,
    limits: Limits,
    events: broadcast::Sender<ExtensionEvent>,
    pending: PendingCalls,
    services: ServiceCalls,
    health: HealthCheck,
    /// Since when the extension has no call. `None` while a call runs.
    idle_since: Option<Instant>,
    phase: Phase,
    /// The callers that wait for the end of the process.
    stop_waiters: Vec<oneshot::Sender<()>>,
    owner: Arc<Mutex<Owner>>,
    /// The lock of the owner, from the stop of an idle process to its end.
    held: Option<OwnedMutexGuard<Owner>>,
}

impl<Owner: Send + 'static> Connection<Owner> {
    /// Starts the process and does the handshake and the initialization.
    /// On a failure, the process has ended when the function returns.
    pub(crate) async fn open(
        setup: Setup,
        owner: Arc<Mutex<Owner>>,
    ) -> Result<(ConnectionHandle, Self), Failure> {
        let spawned = setup.launch.spawn();
        let child = spawned.map_err(|error| Failure::Spawn(error.to_string()))?;
        let process = ExtensionProcess::new(child, setup.stderr_log, &setup.limits)?;
        let (sender, commands) = mpsc::unbounded_channel();
        let request_ids = RequestIds::default();
        let mut connection = Self {
            process,
            commands,
            request_ids: request_ids.clone(),
            extension_id: setup.identity.extension_id.clone(),
            events: setup.events,
            pending: PendingCalls::default(),
            services: ServiceCalls::new(
                setup.services,
                setup.identity.extension_id.clone(),
                setup.identity.granted.scopes.clone(),
            ),
            health: HealthCheck::new(setup.limits.health_interval, setup.limits.health),
            idle_since: None,
            phase: Phase::Greeting,
            stop_waiters: Vec::new(),
            owner,
            held: None,
            limits: setup.limits,
        };
        let started = timeout(connection.limits.start, connection.start(&setup.identity));
        match started.await.unwrap_or(Err(Failure::StartTimeout)) {
            Ok(()) => Ok((ConnectionHandle::new(sender, request_ids), connection)),
            Err(failure) => {
                connection.process.kill().await;
                Err(failure)
            }
        }
    }

    async fn start(&mut self, identity: &Identity) -> Result<(), Failure> {
        let deadline = Instant::now() + self.limits.start;
        handshake::greet(&mut self.process, self.request_ids.next(), identity).await?;
        let request_id = self.request_ids.next();
        let initialize = Body::Initialize(InitializeRequest {});
        self.process
            .send(&envelopes::request(request_id, initialize))
            .await?;
        self.phase = Phase::Initializing {
            request_id,
            deadline,
        };
        // The extension can call host services and send events while it
        // initializes.
        while matches!(self.phase, Phase::Initializing { .. }) {
            self.step().await?;
        }
        Ok(())
    }

    /// Serves the extension and the callers until the process ends. The
    /// result has the lock of the owner if the connection took it.
    pub(crate) async fn serve(mut self) -> (End, Option<OwnedMutexGuard<Owner>>) {
        self.refresh_idle();
        let end = loop {
            if matches!(self.phase, Phase::Ended) {
                break End::Stopped;
            }
            if let Err(failure) = self.step().await {
                break End::Failed(failure);
            }
        };
        self.process.kill().await;
        self.fail_calls(&end);
        (end, self.held)
    }

    /// Waits for one input and handles it.
    async fn step(&mut self) -> Result<(), Failure> {
        let is_serving = matches!(self.phase, Phase::Serving);
        let deadline = self.deadline();
        let input = tokio::select! {
            event = self.process.next() => Input::Process(event),
            Some(command) = self.commands.recv(), if is_serving => Input::Command(command),
            response = self.services.next_response() => Input::ServiceResponse(response),
            () = sleep_until(deadline) => Input::Deadline,
        };
        match input {
            Input::Process(event) => match event? {
                ProcessEvent::Envelope(envelope) => self.on_message(envelope.kind).await,
                ProcessEvent::Exited(status) => match self.phase {
                    Phase::Exiting { .. } if status.success() => {
                        self.phase = Phase::Ended;
                        Ok(())
                    }
                    _ => Err(Failure::Exited(status)),
                },
            },
            Input::Command(command) => self.on_command(command).await,
            Input::ServiceResponse(response) => {
                self.process.send(&response).await?;
                self.refresh_idle();
                Ok(())
            }
            Input::Deadline => self.on_deadline().await,
        }
    }

    /// The time at which the connection must do something without an input.
    fn deadline(&self) -> Instant {
        match self.phase {
            Phase::Initializing { deadline, .. }
            | Phase::ShuttingDown { deadline, .. }
            | Phase::Exiting { deadline } => deadline,
            Phase::Serving => {
                let optional = [self.idle_deadline(), self.pending.cancel_deadline()];
                let earliest_optional = optional.into_iter().flatten().min();
                earliest_optional.map_or(self.health.deadline(), |deadline| {
                    deadline.min(self.health.deadline())
                })
            }
            Phase::Greeting | Phase::Ended => Instant::now(),
        }
    }

    async fn on_deadline(&mut self) -> Result<(), Failure> {
        match self.phase {
            Phase::Greeting | Phase::Initializing { .. } => return Err(Failure::StartTimeout),
            Phase::ShuttingDown { .. } | Phase::Exiting { .. } => {
                return Err(Failure::ShutdownTimeout);
            }
            Phase::Serving | Phase::Ended => {}
        }
        let now = Instant::now();
        let is_due = |deadline: Option<Instant>| deadline.is_some_and(|deadline| deadline <= now);
        if is_due(self.pending.cancel_deadline()) {
            return Err(Failure::CancelTimeout);
        }
        if is_due(Some(self.health.deadline())) {
            self.check_health().await?;
        }
        if is_due(self.idle_deadline()) {
            self.stop_if_idle().await?;
        }
        Ok(())
    }

    async fn check_health(&mut self) -> Result<(), Failure> {
        match self.health.step() {
            HealthStep::Overdue => Err(Failure::HealthTimeout),
            HealthStep::Ask => {
                let request_id = self.request_ids.next();
                let health = Body::Health(HealthRequest {});
                self.process
                    .send(&envelopes::request(request_id, health))
                    .await?;
                self.health.asked(request_id);
                Ok(())
            }
        }
    }

    async fn on_message(&mut self, kind: Option<Kind>) -> Result<(), Failure> {
        match kind {
            Some(Kind::Response(response)) => self.on_response(response),
            Some(Kind::Request(request)) => self.on_request(request).await,
            Some(Kind::Event(event)) => {
                self.publish(event);
                Ok(())
            }
            Some(Kind::Cancel(cancel)) => {
                if let Some(response) = self.services.cancel(cancel.id) {
                    self.process.send(&response).await?;
                }
                self.refresh_idle();
                Ok(())
            }
            // A kind of a newer version of the protocol.
            None => Ok(()),
        }
    }

    fn on_response(&mut self, response: Response) -> Result<(), Failure> {
        match self.phase {
            Phase::Initializing { request_id, .. } if request_id == response.id => {
                return self.on_initialized(response.result);
            }
            Phase::ShuttingDown {
                request_id,
                deadline,
            } if request_id == response.id => {
                self.phase = Phase::Exiting { deadline };
                return Ok(());
            }
            _ => {}
        }
        if !self.health.answered(response.id) {
            self.pending.resolve(response.id, response.result);
            self.refresh_idle();
        }
        Ok(())
    }

    fn on_initialized(&mut self, result: Option<response::Result>) -> Result<(), Failure> {
        match result {
            Some(response::Result::Initialize(_)) => {
                self.phase = Phase::Serving;
                Ok(())
            }
            Some(response::Result::Error(error)) => Err(Failure::InitializeRefused(error.message)),
            _ => Err(Failure::Protocol(
                "the response to `InitializeRequest` has a different result".to_owned(),
            )),
        }
    }

    /// Handles a request of the extension: a call of a host service.
    async fn on_request(&mut self, request: Request) -> Result<(), Failure> {
        let refusal = if let Some(Body::Call(call)) = request.body {
            self.services.start(request.id, call)
        } else {
            let error = ServiceError::failed("the host does not know this request");
            let result = response::Result::Error(error.into());
            Some(envelopes::response(request.id, result))
        };
        if let Some(refusal) = refusal {
            self.process.send(&refusal).await?;
        }
        self.refresh_idle();
        Ok(())
    }

    /// Gives an event to the subscribers. An event that a subscriber gets
    /// is activity: the host does not stop an extension whose events are in
    /// use.
    fn publish(&mut self, event: Event) {
        if self.events.receiver_count() == 0 {
            return;
        }
        let event = ExtensionEvent {
            extension_id: self.extension_id.clone(),
            name: event.name,
            payload: event.payload,
        };
        // The last subscriber can go away at each moment.
        let _ = self.events.send(event);
        if let Some(idle_since) = &mut self.idle_since {
            *idle_since = Instant::now();
        }
    }

    async fn on_command(&mut self, command: Command) -> Result<(), Failure> {
        match command {
            Command::Call {
                id,
                envelope,
                reply,
            } => {
                self.pending.add(id, reply);
                self.refresh_idle();
                self.process.send(&envelope).await
            }
            Command::Cancel { id } => {
                if self.pending.cancel(id, self.limits.cancel) {
                    self.process.send(&envelopes::cancel(id)).await?;
                }
                Ok(())
            }
            Command::Stop { done } => {
                self.stop_waiters.push(done);
                self.begin_shutdown().await
            }
        }
    }

    fn refresh_idle(&mut self) {
        if self.pending.is_empty() && self.services.is_empty() {
            self.idle_since.get_or_insert_with(Instant::now);
        } else {
            self.idle_since = None;
        }
    }

    fn idle_deadline(&self) -> Option<Instant> {
        self.idle_since.map(|since| since + self.limits.idle)
    }

    /// Stops the process if no caller is about to give it a call.
    async fn stop_if_idle(&mut self) -> Result<(), Failure> {
        let lock = Arc::clone(&self.owner).try_lock_owned();
        let (Ok(held), true) = (lock, self.commands.is_empty()) else {
            // A caller has the lock, or its command waits. The idle time
            // starts again.
            self.idle_since = Some(Instant::now());
            return Ok(());
        };
        self.held = Some(held);
        self.begin_shutdown().await
    }

    /// Fails the calls that run and tells the extension to stop.
    async fn begin_shutdown(&mut self) -> Result<(), Failure> {
        for id in self.pending.fail_all(&CallError::Stopped) {
            self.process.send(&envelopes::cancel(id)).await?;
        }
        self.services.abort_all();
        let request_id = self.request_ids.next();
        let shutdown = Body::Shutdown(ShutdownRequest {});
        self.process
            .send(&envelopes::request(request_id, shutdown))
            .await?;
        self.phase = Phase::ShuttingDown {
            request_id,
            deadline: Instant::now() + self.limits.shutdown,
        };
        Ok(())
    }

    /// Fails each call that has no response and each call that a caller
    /// gave after the last step.
    fn fail_calls(&mut self, end: &End) {
        let error = match end {
            End::Stopped => CallError::Stopped,
            End::Failed(failure) => CallError::Failed(failure.clone()),
        };
        self.pending.fail_all(&error);
        self.commands.close();
        while let Ok(command) = self.commands.try_recv() {
            if let Command::Call { reply, .. } = command {
                // A caller that is gone stopped waiting.
                let _ = reply.send(Err(error.clone()));
            }
        }
    }
}
