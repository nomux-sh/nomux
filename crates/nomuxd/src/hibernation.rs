//! The hibernation of idle agent sessions: one task that applies the
//! triggers of the configuration. `docs/memory.md` has the rules.
//!
//! - `policy`: which session can hibernate and which ones a trigger
//!   hibernates. Pure code.
//! - `pressure`: the memory pressure reports of the operating system.
//! - `resume_command`: the words of a resume command, without a shell.
//!
//! The task does not poll. It wakes for an event of the event log (a change
//! of a status, a new session), for a detach, for a pressure report, and at
//! the time when a session becomes idle long enough, only if a trigger can
//! then apply. A daemon without a change wakes no time.

mod policy;
mod pressure;
pub(crate) mod resume_command;

use std::sync::Arc;
use std::time::Duration;

use nomux_holder_protocol::SessionId;
use nomux_status::Timestamp;
use tokio::time::sleep;
use tracing::warn;

pub(crate) use self::policy::{Candidate, Measurement, Policy, Trigger, safety};
use self::pressure::PressureReports;
use crate::clock;
use crate::events::EventLog;
use crate::sessions::Sessions;

/// Applies the triggers of the configuration until the event log closes.
pub(crate) async fn run(sessions: Arc<Sessions>, events: Arc<EventLog>) {
    let policy = sessions.hibernation_policy();
    let check = sessions.hibernation_check();
    let mut pressure = sessions
        .hibernates_on_pressure()
        .then(|| {
            PressureReports::open()
                .inspect_err(|error| {
                    warn!(%error, "the hibernation gets no memory pressure reports");
                })
                .ok()
        })
        .flatten();
    let mut newest = events.watch();
    let mut next_look: Option<Timestamp> = None;
    let mut measured_for: Vec<SessionId> = Vec::new();
    loop {
        let reported = tokio::select! {
            changed = newest.changed() => {
                if changed.is_err() {
                    return;
                }
                None
            }
            () = check.notified() => None,
            report = next_pressure(pressure.as_mut()) => match report {
                Ok(report) => Some(report),
                Err(error) => {
                    warn!(%error, "the memory pressure reports stopped");
                    pressure = None;
                    None
                }
            },
            () = sleep_until(next_look), if next_look.is_some() => None,
        };
        let candidates = sessions.hibernation_candidates();
        let review = policy.review(&candidates, clock::now());
        next_look = review.next_look;
        let eligible: Vec<SessionId> = review.eligible.iter().map(|c| c.id.clone()).collect();
        // The memory is measured when the sessions that can hibernate change,
        // or when the operating system reports pressure.
        let measure = policy.needs_memory()
            && !eligible.is_empty()
            && (eligible != measured_for || reported.is_some());
        let memory = if measure {
            measured_for.clone_from(&eligible);
            sessions.measurement().await
        } else {
            None
        };
        for choice in policy.choose(
            &review.eligible,
            candidates.len(),
            memory.as_ref(),
            reported,
        ) {
            if let Err(error) = sessions.hibernate(&choice.id, choice.trigger).await {
                warn!(session = %choice.id, %error, "the session did not hibernate");
            }
        }
    }
}

async fn next_pressure(
    pressure: Option<&mut PressureReports>,
) -> std::io::Result<policy::Pressure> {
    match pressure {
        Some(pressure) => pressure.next().await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(time: Option<Timestamp>) {
    let now = clock::now();
    let wait = time.map_or(0, |time| time.0.saturating_sub(now.0));
    sleep(Duration::from_millis(wait)).await;
}
