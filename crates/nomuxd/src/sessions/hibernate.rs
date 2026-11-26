//! The hibernation of an agent session and its wake. The rules are in
//! `crate::hibernation`; this module does what they decide.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;

use nomux_core::configuration::StopSignal;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::session_state::SessionMetadata;
use nomux_holder_protocol::wire::ChildSignal;
use nomux_proto::wire::v1::{
    HibernationReason as WireReason, MemoryMeasure as WireMeasure, SessionHibernated,
    SessionMemoryLimitReached, SessionWakeFailed, SessionWoken, event,
};
use tokio::sync::Notify;
use tracing::{info, warn};

use super::{ListedSession, Sessions};
use crate::clock;
use crate::hibernation::{Candidate, Measurement, Policy, Trigger, resume_command, safety};
use crate::holder::SessionEnd;
use crate::memory_limit;
use crate::session_error::SessionError;
use crate::session_handle::SessionHandle;
use crate::session_memory::MemoryMeasure;
use crate::session_registry::SessionState;
use crate::session_store::{HibernationReason, HibernationRecord};

impl Sessions {
    /// The rules of the triggers of the configuration.
    pub(crate) fn hibernation_policy(&self) -> Policy {
        Policy {
            hibernate_after: self.hibernation.hibernate_after,
            max_live: self.hibernation.max_live,
            // A limit that the kernel applies needs no threshold.
            session_threshold: self
                .session_memory_limit
                .filter(|_| !self.limit_support.is_applied()),
            total_threshold: self.total_memory_limit,
        }
    }

    /// What the operating system gives for a memory limit, for
    /// `Host.Info`: if it applies the limit, and the description.
    pub(crate) fn memory_limit_support(&self) -> (bool, String) {
        (self.limit_support.is_applied(), self.limit_support.detail())
    }

    /// The limit that the kernel applies to the command of the session
    /// `id`, in bytes. 0 if it applies none.
    pub(crate) fn kernel_memory_limit(&self, id: &SessionId) -> u64 {
        match self.registry.find(id) {
            Some((record, _)) => match record.state {
                SessionState::Running { pid } => memory_limit::kernel_limit(pid),
                _ => 0,
            },
            None => 0,
        }
    }

    pub(crate) fn hibernates_on_pressure(&self) -> bool {
        self.hibernation.on_pressure
    }

    /// Wakes the hibernation when a fact of its rules changed without an
    /// event.
    pub(crate) fn hibernation_check(&self) -> Arc<Notify> {
        Arc::clone(&self.hibernation_check)
    }

    /// The live agent sessions: each session whose command runs and whose
    /// adapter gave details.
    pub(crate) fn hibernation_candidates(&self) -> Vec<Candidate> {
        self.registry
            .records()
            .into_iter()
            .filter(|record| matches!(record.state, SessionState::Running { .. }))
            .filter_map(|record| {
                let handle = self.registry.find(&record.id)?.1?;
                self.candidate(&record.id, &handle)
            })
            .collect()
    }

    /// The memory of the process tree of the command of each live session.
    pub(crate) async fn measurement(&self) -> Option<Measurement> {
        match self.measure(None).await {
            Ok(sessions) => Some(Measurement {
                total: sessions.iter().map(|memory| memory.command_bytes).sum(),
                sessions: sessions
                    .into_iter()
                    .map(|memory| (memory.id, memory.command_bytes))
                    .collect::<HashMap<_, _>>(),
            }),
            Err(error) => {
                warn!(%error, "the hibernation cannot measure the memory of the sessions");
                None
            }
        }
    }

    /// Hibernates the session `id` for a client: the conditions of each
    /// hibernation apply, and the idle time and the pin do not.
    pub(crate) async fn hibernate_on_request(
        &self,
        id: &str,
    ) -> Result<ListedSession, SessionError> {
        let cannot = |reason: &dyn std::fmt::Display| {
            SessionError::Failed(format!("session {id} cannot hibernate: {reason}"))
        };
        let (record, handle) = self.find(id)?;
        let (SessionState::Running { .. }, Some(handle)) = (record.state, handle) else {
            return Err(cannot(&"its command does not run"));
        };
        let candidate = self
            .candidate(&record.id, &handle)
            .ok_or_else(|| cannot(&"no adapter reports it as an agent session"))?;
        safety(&candidate).map_err(|block| cannot(&block))?;
        self.hibernate(&record.id, Trigger::Request).await
    }

    /// Writes the hibernation of the session `id`, ends its command and
    /// keeps it as a hibernated session. The caller checked the rules.
    pub(crate) async fn hibernate(
        &self,
        id: &SessionId,
        trigger: Trigger,
    ) -> Result<ListedSession, SessionError> {
        let handle = self.find_reachable(id.as_str())?;
        let resume_command = self
            .status
            .agent_facts(id)
            .map(|facts| facts.resume_command)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| {
                SessionError::Failed(format!(
                    "session {id} cannot hibernate: its adapter gave no resume command"
                ))
            })?;
        let freed = self
            .measure(Some(id))
            .await
            .ok()
            .and_then(|sessions| sessions.into_iter().next())
            .map(|memory| (memory.command_bytes, memory.measure));
        let record = HibernationRecord {
            hibernated_at: clock::now_seconds(),
            reason: reason_of(trigger),
            freed,
            resume_command,
            woken_at: None,
            wake_error: None,
        };
        // A daemon that starts after the stop reads the file: the session is
        // hibernated, not ended.
        self.store.set_hibernation(id, &record).map_err(|error| {
            SessionError::Failed(format!(
                "session {id} cannot hibernate: its hibernation cannot be stored: {error}"
            ))
        })?;
        let signals = self
            .hibernation
            .stop_signals
            .iter()
            .copied()
            .map(child_signal);
        if let Err(error) = handle
            .hibernate(signals.collect(), self.hibernation.stop_wait)
            .await
        {
            if let Err(error) = self.store.remove_hibernation(id) {
                warn!(session = %id, %error, "cannot remove the hibernation that failed");
            }
            return Err(error);
        }
        info!(session = %id, ?trigger, ?freed, "the session hibernated");
        self.events
            .publish(event::Body::SessionHibernated(hibernated(
                id, trigger, freed,
            )));
        let (record, _) = self.find(id.as_str())?;
        Ok(self.describe(record))
    }

    /// Excludes the session `id` from automatic hibernation, or takes the
    /// exclusion back.
    pub(crate) fn set_pinned(&self, id: &str, pinned: bool) -> Result<(), SessionError> {
        let (record, _) = self.find(id)?;
        if record.state == SessionState::Damaged {
            return Err(super::damaged(&record));
        }
        self.store.set_pinned(&record.id, pinned).map_err(|error| {
            SessionError::Failed(format!("the pin of session {id} cannot be stored: {error}"))
        })?;
        if !pinned {
            self.hibernation_check.notify_one();
        }
        Ok(())
    }

    /// The words of the resume command that wakes the hibernated session
    /// `id`, as a command for its holder.
    ///
    /// # Errors
    ///
    /// Returns the failed wake if the command cannot start.
    pub(super) fn wake_command(
        &self,
        id: &SessionId,
        record: &HibernationRecord,
        metadata: &SessionMetadata,
    ) -> Result<Vec<OsString>, SessionError> {
        let words = resume_command::words(&record.resume_command)
            .map_err(|error| self.wake_failed(id, record, metadata, &error.to_string()))?;
        if nomux_core::program_in_path(&words[0]).is_none() {
            let reason = format!("the program {} is not in PATH", words[0]);
            return Err(self.wake_failed(id, record, metadata, &reason));
        }
        Ok(words.into_iter().map(OsString::from).collect())
    }

    /// The wake of `id` failed for `reason`: the session stays hibernated,
    /// the record and an event say why.
    pub(super) fn wake_failed(
        &self,
        id: &SessionId,
        record: &HibernationRecord,
        metadata: &SessionMetadata,
        reason: &str,
    ) -> SessionError {
        let failed = HibernationRecord {
            wake_error: Some(reason.to_owned()),
            ..record.clone()
        };
        if let Err(error) = self.store.set_hibernation(id, &failed) {
            warn!(session = %id, %error, "cannot store the failed wake");
        }
        warn!(session = %id, reason, "the wake of the session failed");
        self.events
            .publish(event::Body::SessionWakeFailed(SessionWakeFailed {
                session_id: id.to_string(),
                error: reason.to_owned(),
                resume_command: record.resume_command.clone(),
            }));
        SessionError::Failed(format!(
            "session {id} did not wake: {reason}. To continue its agent by hand, run this \
             command in {}: {}",
            metadata.working_directory.display(),
            record.resume_command
        ))
    }

    /// The resume command of `id` started: the run is the resumed run.
    pub(super) fn woken(&self, id: &SessionId, record: HibernationRecord) {
        let resume_command = record.resume_command.clone();
        let woken = HibernationRecord {
            woken_at: Some(clock::now_seconds()),
            wake_error: None,
            ..record
        };
        if let Err(error) = self.store.set_hibernation(id, &woken) {
            warn!(session = %id, %error, "cannot store the wake of the session");
        }
        self.events.publish(event::Body::SessionWoken(SessionWoken {
            session_id: id.to_string(),
            resume_command,
        }));
    }

    /// The words that start a command of the session `id` in the epoch
    /// `epoch` with the memory limit of the configuration. Empty if the
    /// operating system applies no limit.
    pub(super) fn launcher(&self, id: &SessionId, epoch: u32) -> Vec<OsString> {
        self.session_memory_limit.map_or_else(Vec::new, |bytes| {
            self.limit_support.launcher(id, epoch, bytes)
        })
    }

    /// Tells the clients when the kernel ends the session `id` at its
    /// memory limit.
    pub(super) fn report_memory_limit(&self, handle: &SessionHandle) {
        let Some(limit) = self
            .session_memory_limit
            .filter(|_| self.limit_support.is_applied())
        else {
            return;
        };
        let id = handle.id().clone();
        let mut ended = handle.ended.clone();
        let store = Arc::clone(&self.store);
        let events = Arc::clone(&self.events);
        tokio::spawn(async move {
            let Ok(end) = ended.wait_for(Option::is_some).await.map(|end| *end) else {
                return;
            };
            // The kernel ends the processes of the scope with `SIGKILL`.
            if end != Some(SessionEnd::Signaled { signal: 9 }) {
                return;
            }
            let Ok(metadata) = store.metadata(&id) else {
                return;
            };
            if memory_limit::limit_was_reached(&id, metadata.epoch).await {
                warn!(session = %id, limit, "the kernel ended the session at its memory limit");
                events.publish(event::Body::SessionMemoryLimitReached(
                    SessionMemoryLimitReached {
                        session_id: id.to_string(),
                        limit_bytes: limit,
                    },
                ));
            }
        });
    }

    fn candidate(&self, id: &SessionId, handle: &SessionHandle) -> Option<Candidate> {
        let facts = self.status.agent_facts(id)?;
        let activity = handle.activity();
        Some(Candidate {
            id: id.clone(),
            state: facts.aggregate.state,
            since: facts.aggregate.since,
            can_resume: !facts.resume_command.is_empty(),
            attached: handle.clients.count() > 0,
            pinned: self.store.is_pinned(id),
            last_input: activity.last_input,
            last_detach: activity.last_detach,
        })
    }
}

fn child_signal(signal: StopSignal) -> ChildSignal {
    match signal {
        StopSignal::Hangup => ChildSignal::Hangup,
        StopSignal::Interrupt => ChildSignal::Interrupt,
        StopSignal::Terminate => ChildSignal::Terminate,
        StopSignal::Kill => ChildSignal::Kill,
    }
}

fn reason_of(trigger: Trigger) -> HibernationReason {
    match trigger {
        Trigger::Count { .. } => HibernationReason::Count,
        Trigger::SessionMemory { .. } => HibernationReason::SessionMemory,
        Trigger::TotalMemory { .. } => HibernationReason::TotalMemory,
        Trigger::MemoryPressure(_) => HibernationReason::MemoryPressure,
        Trigger::Request => HibernationReason::Request,
    }
}

pub(crate) fn wire_reason(reason: HibernationReason) -> WireReason {
    match reason {
        HibernationReason::Count => WireReason::Count,
        HibernationReason::SessionMemory => WireReason::SessionMemory,
        HibernationReason::TotalMemory => WireReason::TotalMemory,
        HibernationReason::MemoryPressure => WireReason::MemoryPressure,
        HibernationReason::Request => WireReason::Request,
    }
}

pub(crate) fn wire_measure(measure: MemoryMeasure) -> WireMeasure {
    match measure {
        MemoryMeasure::Proportional => WireMeasure::Proportional,
        MemoryMeasure::Resident => WireMeasure::Resident,
    }
}

fn hibernated(
    id: &SessionId,
    trigger: Trigger,
    freed: Option<(u64, MemoryMeasure)>,
) -> SessionHibernated {
    let mut event = SessionHibernated {
        session_id: id.to_string(),
        reason: wire_reason(reason_of(trigger)).into(),
        freed_bytes: freed.map_or(0, |(bytes, _)| bytes),
        measure: freed
            .map_or(WireMeasure::Unspecified, |(_, measure)| {
                wire_measure(measure)
            })
            .into(),
        ..SessionHibernated::default()
    };
    match trigger {
        Trigger::Count { live, max_live } => {
            event.live_agents = u32::try_from(live).unwrap_or(u32::MAX);
            event.max_live_agents = u32::try_from(max_live).unwrap_or(u32::MAX);
        }
        Trigger::SessionMemory { bytes, limit } | Trigger::TotalMemory { bytes, limit } => {
            event.memory_bytes = bytes;
            event.memory_limit_bytes = limit;
        }
        Trigger::MemoryPressure(pressure) => {
            let milliseconds = |duration: std::time::Duration| {
                u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
            };
            event.pressure_stall_ms = milliseconds(pressure.stall);
            event.pressure_window_ms = milliseconds(pressure.window);
        }
        Trigger::Request => {}
    }
    event
}
