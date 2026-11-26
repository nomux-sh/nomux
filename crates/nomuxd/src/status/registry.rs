//! The status of each session, outside the session registry.
//!
//! An entry has what each source reports for its session and the Agent run
//! that the resolver made from it. A change of a source resolves the Agent
//! run again, and each change of the Agent run goes to the event log.
//!
//! The memory of an entry has a limit: the holder keeps 64 program status
//! records at most with a limit for each field, the resolver keeps
//! [`nomux_status::MAX_CHILD_WORK`] Child work at most, and a message has
//! [`MAX_MESSAGE_LENGTH`](super::holder_sources::MAX_MESSAGE_LENGTH) bytes
//! at most. Only a session of the session registry gets an entry.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::program_reports::ProgramReports;
use nomux_proto::wire::v1::{
    AgentExplainResponse, AgentRunChanged, AgentRunRemoved, AgentTreeResponse, event,
};
use nomux_status::{
    AgentRun, Observation, Source, SourceReport, carried_over, most_urgent, resolve,
};

use super::foreground;
use super::holder_sources::{program_status, prompt_marks};
use super::message;
use super::self_report::{SelfReport, SelfReportRefused};
use crate::clock;
use crate::events::EventLog;
use crate::holder::HolderFacts;

/// The status of each session.
pub(crate) struct StatusRegistry {
    entries: Mutex<HashMap<SessionId, Entry>>,
    events: Arc<EventLog>,
}

/// The status of one session.
struct Entry {
    run: AgentRun,
    /// The holder of the session. `None` if the session has no process.
    holder: Option<HolderFacts>,
    sources: Sources,
}

/// What the sources of one session report, without the foreground
/// program: the daemon reads it when it resolves.
#[derive(Default)]
struct Sources {
    program_status: Option<SourceReport>,
    prompt_marks: Option<SourceReport>,
    self_report: Option<Observation>,
    carried_over: Option<SourceReport>,
}

impl StatusRegistry {
    pub(crate) fn new(events: Arc<EventLog>) -> Self {
        Self {
            entries: Mutex::default(),
            events,
        }
    }

    /// The status task of the session `id` has a connection to its holder.
    /// A different holder than before is a new process: its sources start
    /// empty, and the status comes from them.
    pub(crate) fn connected(&self, id: &SessionId, facts: HolderFacts) {
        self.change(id, |entry| {
            let same_process = entry.holder.is_some_and(|known| {
                known.child_pid == facts.child_pid && known.token == facts.token
            });
            if !same_process {
                entry.sources = Sources::default();
            }
            entry.holder = Some(facts);
        });
    }

    /// The holder of the session `id` has new program reports.
    pub(crate) fn program_reports(&self, id: &SessionId, reports: &ProgramReports) {
        self.change(id, |entry| {
            entry.sources.program_status = Some(program_status(reports));
            entry.sources.prompt_marks = Some(prompt_marks(&reports.input_line));
        });
    }

    /// A program reports the status of the session `id` with the token
    /// `token`.
    ///
    /// # Errors
    ///
    /// Returns the reason if the token does not prove that the request comes
    /// from a process of the session.
    pub(crate) fn self_report(
        &self,
        id: &SessionId,
        token: &str,
        report: SelfReport,
    ) -> Result<(), SelfReportRefused> {
        let mut entries = self.entries();
        let entry = entries.get_mut(id).ok_or(SelfReportRefused::NoProcess)?;
        let holder = entry.holder.ok_or(SelfReportRefused::NoProcess)?;
        let known = holder.token.ok_or(SelfReportRefused::HolderWithoutToken)?;
        if !known.matches(token) {
            return Err(SelfReportRefused::WrongToken);
        }
        entry.sources.self_report = match report {
            SelfReport::Status(observation) => Some(observation),
            SelfReport::Withdrawn => None,
        };
        self.resolve_and_publish(id, entry);
        Ok(())
    }

    /// The session `id` lost the process of the holder `facts` and is
    /// dormant now. Nothing changes if a newer holder has the session.
    pub(crate) fn lost_process(&self, id: &SessionId, facts: HolderFacts) {
        let mut entries = self.entries();
        let Some(entry) = entries.get_mut(id) else {
            return;
        };
        let is_current = entry
            .holder
            .is_some_and(|known| known.child_pid == facts.child_pid && known.token == facts.token);
        if !is_current {
            return;
        }
        entry.holder = None;
        entry.sources = Sources {
            carried_over: carried_over(&entry.run),
            ..Sources::default()
        };
        self.resolve_and_publish(id, entry);
    }

    /// The session `id` ended or was removed.
    pub(crate) fn remove(&self, id: &SessionId) {
        if self.entries().remove(id).is_some() {
            self.events
                .publish(event::Body::AgentRunRemoved(AgentRunRemoved {
                    session_id: id.to_string(),
                }));
        }
    }

    /// The user looks at the session `id`: each result of it is seen.
    pub(crate) fn mark_seen(&self, id: &SessionId) {
        let mut entries = self.entries();
        if let Some(entry) = entries.get_mut(id)
            && entry.run.mark_seen()
        {
            self.publish_changed(id, &entry.run);
        }
    }

    /// The status of the sessions `ids` and the most urgent status of
    /// them.
    pub(crate) fn tree(&self, ids: &[SessionId], include_children: bool) -> AgentTreeResponse {
        let mut entries = self.entries();
        let runs: Vec<(&SessionId, AgentRun)> = ids
            .iter()
            .map(|id| (id, Self::read(&mut entries, id).run.clone()))
            .collect();
        let attention = most_urgent(runs.iter().map(|(id, run)| (*id, &run.aggregate)));
        AgentTreeResponse {
            attention_session_id: attention.map_or_default(|(id, _)| id.to_string()),
            aggregate: attention.map(|(_, status)| message::status(status)),
            runs: runs
                .iter()
                .map(|(id, run)| message::agent_run(id, run, include_children))
                .collect(),
        }
    }

    /// The status of the session `id` and what each source says.
    pub(crate) fn explain(&self, id: &SessionId) -> AgentExplainResponse {
        let mut entries = self.entries();
        let entry = Self::read(&mut entries, id);
        AgentExplainResponse {
            run: Some(message::agent_run(id, &entry.run, true)),
            sources: entry.reports().iter().map(message::source_report).collect(),
        }
    }

    /// The Agent run of each of the sessions `ids` and the `seq` of the
    /// newest event that they include, for a subscriber that starts with a
    /// snapshot.
    pub(crate) fn snapshot(&self, ids: &[SessionId]) -> (u64, Vec<event::Body>) {
        let mut entries = self.entries();
        // An event comes from a change under this lock, thus no event is
        // between the runs and the number.
        let last_seq = self.events.last_seq();
        let runs = ids
            .iter()
            .map(|id| {
                let run = &Self::read(&mut entries, id).run;
                changed(id, run)
            })
            .collect();
        (last_seq, runs)
    }

    /// The entry of the session `id` after a read of the foreground
    /// program. The operating system does not tell the daemon a change of
    /// the foreground, thus such a change makes no event.
    fn read<'entries>(
        entries: &'entries mut HashMap<SessionId, Entry>,
        id: &SessionId,
    ) -> &'entries Entry {
        let entry = entries.entry(id.clone()).or_insert_with(Entry::unreported);
        entry.run = resolve(&entry.reports(), Some(&entry.run), clock::now());
        entry
    }

    /// Changes the entry of the session `id`, and resolves its Agent run
    /// again.
    fn change(&self, id: &SessionId, change: impl FnOnce(&mut Entry)) {
        let mut entries = self.entries();
        let entry = entries.entry(id.clone()).or_insert_with(Entry::unreported);
        change(entry);
        self.resolve_and_publish(id, entry);
    }

    fn resolve_and_publish(&self, id: &SessionId, entry: &mut Entry) {
        let run = resolve(&entry.reports(), Some(&entry.run), clock::now());
        if run != entry.run {
            entry.run = run;
            self.publish_changed(id, &entry.run);
        }
    }

    fn publish_changed(&self, id: &SessionId, run: &AgentRun) {
        self.events.publish(changed(id, run));
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<SessionId, Entry>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Entry {
    fn unreported() -> Self {
        Self {
            run: AgentRun::unreported(clock::now()),
            holder: None,
            sources: Sources::default(),
        }
    }

    /// The report of each source, in the sequence in which they decide.
    fn reports(&self) -> Vec<SourceReport> {
        let no_process = "the session has no process";
        let held = |report: &Option<SourceReport>, source, note| {
            report
                .clone()
                .unwrap_or_else(|| SourceReport::silent(source, note))
        };
        let program_note = if self.holder.is_some() {
            "the holder gave no program reports"
        } else {
            no_process
        };
        vec![
            held(
                &self.sources.program_status,
                Source::ProgramStatus,
                program_note,
            ),
            self.self_report(),
            held(
                &self.sources.carried_over,
                Source::SessionRestore,
                "the session did not lose a process with work that ran or waited",
            ),
            held(
                &self.sources.prompt_marks,
                Source::PromptMarks,
                program_note,
            ),
            match self.holder {
                Some(holder) => foreground::report(holder.child_pid),
                None => SourceReport::silent(Source::ForegroundProgram, no_process),
            },
        ]
    }

    fn self_report(&self) -> SourceReport {
        match &self.sources.self_report {
            Some(observation) => SourceReport {
                source: Source::SelfReport,
                root: Some(observation.clone()),
                children: Vec::new(),
                note: "a program of the session reported it with `nomux status set`".to_owned(),
            },
            None => SourceReport::silent(
                Source::SelfReport,
                "no program of the session reported a status with `nomux status set`",
            ),
        }
    }
}

fn changed(id: &SessionId, run: &AgentRun) -> event::Body {
    event::Body::AgentRunChanged(Box::new(AgentRunChanged {
        run: Some(message::agent_run(id, run, true)),
    }))
}

#[cfg(test)]
mod tests {
    use nomux_holder_protocol::program_reports::{InputLine, ProgramState, ProgramStatus};
    use nomux_holder_protocol::wire::SessionToken;
    use nomux_proto::wire::v1::{AgentState, StatusSource};
    use nomux_status::State;

    use super::*;
    use crate::events::Read;

    /// No process has this ID, thus the foreground program says nothing.
    const NO_PROCESS: u32 = u32::MAX - 1;

    fn registry() -> StatusRegistry {
        StatusRegistry::new(Arc::new(EventLog::default()))
    }

    fn session() -> SessionId {
        "s1".parse().unwrap()
    }

    fn holder(token: Option<SessionToken>, child_pid: u32) -> HolderFacts {
        HolderFacts { child_pid, token }
    }

    fn token(byte: u8) -> SessionToken {
        SessionToken::from_random([byte; SessionToken::RANDOM_LENGTH])
    }

    fn working(state: ProgramState) -> ProgramReports {
        ProgramReports {
            revision: 1,
            input_line: InputLine::default(),
            statuses: vec![ProgramStatus {
                id: String::new(),
                state,
                kind: None,
                progress: None,
                app: None,
                title: None,
                message: None,
                revision: 1,
            }],
            notifications: Vec::new(),
        }
    }

    fn state_of(registry: &StatusRegistry, id: &SessionId) -> (AgentState, StatusSource) {
        let run = registry.explain(id).run.unwrap();
        let status = run.status.unwrap();
        (status.state(), status.source())
    }

    fn report(state: State) -> SelfReport {
        SelfReport::new(state, "").unwrap()
    }

    #[test]
    fn only_the_token_of_the_current_holder_is_accepted() {
        let registry = registry();
        let id = session();
        assert_eq!(
            registry.self_report(&id, "", report(State::Working)),
            Err(SelfReportRefused::NoProcess)
        );

        registry.connected(&id, holder(None, NO_PROCESS));
        assert_eq!(
            registry.self_report(&id, "", report(State::Working)),
            Err(SelfReportRefused::HolderWithoutToken)
        );

        registry.connected(&id, holder(Some(token(1)), NO_PROCESS));
        assert_eq!(
            registry.self_report(&id, token(2).as_str(), report(State::Working)),
            Err(SelfReportRefused::WrongToken)
        );
        assert_eq!(
            registry.self_report(&id, token(1).as_str(), report(State::Working)),
            Ok(())
        );
        assert_eq!(
            state_of(&registry, &id),
            (AgentState::Working, StatusSource::SelfReport)
        );

        // A new holder of the session has a new token: the old one proves
        // nothing, and the report of the old process is gone.
        registry.connected(&id, holder(Some(token(3)), NO_PROCESS - 1));
        assert_eq!(
            registry.self_report(&id, token(1).as_str(), report(State::Done)),
            Err(SelfReportRefused::WrongToken)
        );
        assert_eq!(
            state_of(&registry, &id),
            (AgentState::Idle, StatusSource::None)
        );
    }

    #[test]
    fn a_lost_process_leaves_done_work_and_a_late_end_of_an_old_holder_changes_nothing() {
        let registry = registry();
        let id = session();
        let old = holder(Some(token(1)), NO_PROCESS);
        registry.connected(&id, old);
        registry.program_reports(&id, &working(ProgramState::Working));

        registry.lost_process(&id, old);
        assert_eq!(
            state_of(&registry, &id),
            (AgentState::Done, StatusSource::SessionRestore)
        );
        assert_eq!(
            registry.self_report(&id, token(1).as_str(), report(State::Working)),
            Err(SelfReportRefused::NoProcess)
        );

        // A restore: the new process reports, then the status task of the
        // old holder ends late.
        let new = holder(Some(token(2)), NO_PROCESS - 1);
        registry.connected(&id, new);
        registry.program_reports(&id, &working(ProgramState::Blocked));
        registry.lost_process(&id, old);
        assert_eq!(
            state_of(&registry, &id),
            (AgentState::NeedsYou, StatusSource::ProgramStatus)
        );
    }

    #[test]
    fn each_change_is_an_event_and_a_look_at_a_result_is_one_more() {
        let log = Arc::new(EventLog::default());
        let registry = StatusRegistry::new(Arc::clone(&log));
        let id = session();
        registry.connected(&id, holder(Some(token(1)), NO_PROCESS));
        registry.program_reports(&id, &working(ProgramState::Working));
        // The same reports again are no change.
        registry.program_reports(&id, &working(ProgramState::Working));
        registry.program_reports(&id, &working(ProgramState::Done));
        registry.mark_seen(&id);
        registry.mark_seen(&id);
        registry.remove(&id);

        let Read::Events(events) = log.read(1, 10) else {
            panic!("the log has the events");
        };
        let kinds: Vec<&str> = events
            .iter()
            .map(|event| match &event.body {
                Some(event::Body::AgentRunChanged(changed)) => {
                    let status = changed.run.as_ref().unwrap().status.as_ref().unwrap();
                    match (status.state(), status.seen) {
                        (AgentState::Working, _) => "working",
                        (AgentState::Done, false) => "done",
                        (AgentState::Done, true) => "seen",
                        _ => "other",
                    }
                }
                Some(event::Body::AgentRunRemoved(_)) => "removed",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["working", "done", "seen", "removed"]);
    }
}
