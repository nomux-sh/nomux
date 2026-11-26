//! The status of each session, outside the session registry.
//!
//! An entry has what each source reports for its session and the Agent run
//! that the resolver made from it. A change of a source resolves the Agent
//! run again, and each change of the Agent run goes to the event log.
//!
//! The memory of an entry has a limit: the holder keeps 64 program status
//! records at most with a limit for each field, the adapter source and the
//! resolver keep [`nomux_status::MAX_CHILD_WORK`] Child work at most, and a
//! text has [`MAX_MESSAGE_LENGTH`](super::holder_sources::MAX_MESSAGE_LENGTH)
//! bytes at most. Only a session of the session registry and a session
//! whose holder the daemon starts now get an entry, and [`MAX_KEPT_RUNS`]
//! ended sessions keep one at most.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::program_reports::ProgramReports;
use nomux_holder_protocol::wire::SessionToken;
use nomux_proto::wire::v1::{
    AgentDetails, AgentExplainResponse, AgentRunChanged, AgentRunRemoved, AgentTreeResponse, event,
};
use nomux_status::{
    AdapterSource, AgentRun, Observation, Source, SourceReport, Status, Timestamp, carried_over,
    most_urgent, resolve,
};

use super::adapter_report::{AdapterReportRefused, ChildReport, RunReport};
use super::foreground;
use super::holder_sources::{program_status, prompt_marks};
use super::message;
use super::self_report::SelfReport;
use super::token_check::TokenRefused;
use crate::clock;
use crate::events::EventLog;
use crate::holder::HolderFacts;

/// The largest number of ended sessions that keep their last Agent run.
/// With the limits of one Agent run, the kept runs use a few MiB at most.
pub(crate) const MAX_KEPT_RUNS: usize = 64;

/// What the hibernation needs to know about the Agent run of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentFacts {
    /// The aggregate: the most urgent status of the run and its Child work.
    pub(crate) aggregate: Status,
    /// The resume command of the adapter. Empty if it gave none.
    pub(crate) resume_command: String,
}

/// The status of each session.
pub(crate) struct StatusRegistry {
    entries: Mutex<HashMap<SessionId, Entry>>,
    events: Arc<EventLog>,
}

/// The status of one session.
struct Entry {
    run: AgentRun,
    /// The holder of the session. `None` if the daemon has no connection
    /// to the holder of a process of the session.
    holder: Option<HolderFacts>,
    /// The token that the daemon gave to the holder that it starts now.
    /// The command of the session can report with it before the daemon has
    /// a connection to the holder.
    starting: Option<SessionToken>,
    /// When the command of the session ended. `Some` only for the kept run
    /// of an ended session.
    ended: Option<Timestamp>,
    sources: Sources,
}

/// What the sources of one session report, without the foreground
/// program: the daemon reads it when it resolves.
#[derive(Default)]
struct Sources {
    adapter: AdapterSource,
    /// What the adapter says about the Agent run beyond its status.
    details: Option<AgentDetails>,
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

    /// The daemon starts a holder for the session `id` and gives it
    /// `token`. This is a new process: its sources start empty, and from
    /// now a request with the token comes from the session.
    pub(crate) fn starting(&self, id: &SessionId, token: SessionToken) {
        self.change(id, |entry| {
            entry.sources = Sources::default();
            entry.holder = None;
            entry.starting = Some(token);
        });
    }

    /// The holder that the daemon started for the session `id` did not
    /// start: its token proves nothing.
    pub(crate) fn not_started(&self, id: &SessionId) {
        if let Some(entry) = self.entries().get_mut(id) {
            entry.starting = None;
        }
    }

    /// `true` while the daemon starts a holder for the session `id` and has
    /// no connection to it.
    pub(crate) fn is_starting(&self, id: &SessionId) -> bool {
        self.entries()
            .get(id)
            .is_some_and(|entry| entry.starting.is_some())
    }

    /// The status task of the session `id` has a connection to its holder.
    /// A different holder than before is a new process: its sources start
    /// empty, and the status comes from them. The holder that the daemon
    /// started has the sources since its start.
    pub(crate) fn connected(&self, id: &SessionId, facts: HolderFacts) {
        self.change(id, |entry| {
            let same_process = entry.holder.is_some_and(|known| {
                known.child_pid == facts.child_pid && known.token == facts.token
            });
            let started = entry.starting.is_some() && entry.starting == facts.token;
            if !same_process && !started {
                entry.sources = Sources::default();
            }
            entry.holder = Some(facts);
            entry.starting = None;
        });
    }

    /// The holder of the session `id` has new program reports.
    pub(crate) fn program_reports(&self, id: &SessionId, reports: &ProgramReports) {
        self.change(id, |entry| {
            entry.sources.program_status = Some(program_status(reports));
            entry.sources.prompt_marks = Some(prompt_marks(&reports.input_line));
        });
    }

    /// Examines if `token` proves that a request comes from a process of
    /// the session `id`.
    ///
    /// # Errors
    ///
    /// Returns the reason if the token does not prove it.
    pub(crate) fn authenticate(&self, id: &SessionId, token: &str) -> Result<(), TokenRefused> {
        let entries = self.entries();
        let entry = entries.get(id).ok_or(TokenRefused::NoProcess)?;
        Self::check_token(entry, token)
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
    ) -> Result<(), TokenRefused> {
        let mut entries = self.entries();
        let entry = entries.get_mut(id).ok_or(TokenRefused::NoProcess)?;
        Self::check_token(entry, token)?;
        entry.sources.self_report = match report {
            SelfReport::Status(observation) => Some(observation),
            SelfReport::Withdrawn => None,
        };
        self.resolve_and_publish(id, entry);
        Ok(())
    }

    /// An adapter extension reports the Agent run of the session `id`.
    ///
    /// # Errors
    ///
    /// Returns the reason if the session has no process.
    pub(crate) fn adapter_run(
        &self,
        id: &SessionId,
        report: RunReport,
    ) -> Result<(), AdapterReportRefused> {
        self.change_adapter(id, |sources| {
            sources.adapter.report_run(report.observation);
            if report.details.is_some() {
                sources.details = report.details;
            }
            Ok(())
        })
    }

    /// An adapter extension reports a Child work of the session `id`.
    ///
    /// # Errors
    ///
    /// Returns the reason if the session has no process or if the report
    /// does not fit its Child work.
    pub(crate) fn adapter_child(
        &self,
        id: &SessionId,
        report: ChildReport,
    ) -> Result<(), AdapterReportRefused> {
        self.change_adapter(id, |sources| {
            let adapter = &mut sources.adapter;
            let reported = adapter.report_child(&report.id, report.change, report.observation);
            reported.map_err(AdapterReportRefused::Refused)
        })
    }

    /// The session `id` lost the process of the holder `facts` and is
    /// dormant now. Nothing changes if a newer holder has the session.
    pub(crate) fn lost_process(&self, id: &SessionId, facts: HolderFacts) {
        let mut entries = self.entries();
        if let Some(entry) = Self::current(&mut entries, id, facts) {
            entry.keep_what_stays();
            self.resolve_and_publish(id, entry);
        }
    }

    /// The command of the session `id`, which the holder `facts` ran,
    /// ended. The session keeps what stays of its Agent run, as a dormant
    /// session does, until the session is removed. If more than
    /// [`MAX_KEPT_RUNS`] ended sessions keep a run, the oldest one loses it.
    pub(crate) fn ended(&self, id: &SessionId, facts: HolderFacts) {
        let mut entries = self.entries();
        let Some(entry) = Self::current(&mut entries, id, facts) else {
            return;
        };
        entry.keep_what_stays();
        if entry.sources.carried_over.is_none() && entry.sources.details.is_none() {
            drop(entries);
            self.remove(id);
            return;
        }
        entry.ended = Some(clock::now());
        self.resolve_and_publish(id, entry);
        let kept = entries.values().filter(|entry| entry.ended.is_some());
        if kept.count() > MAX_KEPT_RUNS
            && let Some(oldest) = Self::oldest_kept(&entries)
        {
            drop(entries);
            self.remove(&oldest);
        }
    }

    /// The sessions `ids` that are not ended, and each ended session that
    /// keeps a run, in the sequence of their identifiers.
    pub(crate) fn with_kept_runs(&self, mut ids: Vec<SessionId>) -> Vec<SessionId> {
        let entries = self.entries();
        let kept = entries.iter().filter(|(_, entry)| entry.ended.is_some());
        ids.extend(kept.map(|(id, _)| id.clone()));
        ids.sort_unstable_by(|first, second| first.as_str().cmp(second.as_str()));
        ids
    }

    /// The kept run of the ended session `id` and what each source said.
    /// Returns `None` if the session keeps no run.
    pub(crate) fn explain_kept(&self, id: &SessionId) -> Option<AgentExplainResponse> {
        let entries = self.entries();
        let entry = entries.get(id).filter(|entry| entry.ended.is_some())?;
        Some(entry.explanation(id))
    }

    /// The session `id` has no Agent run any more: it was removed, it
    /// ended without a run to keep, or its kept run is the oldest one.
    pub(crate) fn remove(&self, id: &SessionId) {
        if self.entries().remove(id).is_some() {
            self.events
                .publish(event::Body::AgentRunRemoved(AgentRunRemoved {
                    session_id: id.to_string(),
                }));
        }
    }

    /// The facts of the Agent run of the session `id`, if an adapter gave
    /// its details: the adapter knows the session as an agent session.
    /// `None` until the status task has a connection to the current holder:
    /// before that, the entry has the run of the process before a restore.
    pub(crate) fn agent_facts(&self, id: &SessionId) -> Option<AgentFacts> {
        let entries = self.entries();
        let entry = entries.get(id).filter(|entry| entry.holder.is_some())?;
        let details = entry.sources.details.as_ref()?;
        Some(AgentFacts {
            aggregate: entry.run.aggregate.clone(),
            resume_command: details.resume_command.clone(),
        })
    }

    /// The user looks at the session `id`: each result of it is seen.
    pub(crate) fn mark_seen(&self, id: &SessionId) {
        let mut entries = self.entries();
        if let Some(entry) = entries.get_mut(id)
            && entry.run.mark_seen()
        {
            self.publish_changed(id, entry);
        }
    }

    /// The status of the sessions `ids` and the most urgent status of
    /// them.
    pub(crate) fn tree(&self, ids: &[SessionId], include_children: bool) -> AgentTreeResponse {
        let mut entries = self.entries();
        for id in ids {
            Self::read(&mut entries, id);
        }
        let runs: Vec<(&SessionId, &Entry)> = ids.iter().map(|id| (id, &entries[id])).collect();
        let attention = most_urgent(runs.iter().map(|(id, entry)| (*id, &entry.run.aggregate)));
        AgentTreeResponse {
            attention_session_id: attention.map_or_default(|(id, _)| id.to_string()),
            aggregate: attention.map(|(_, status)| message::status(status)),
            runs: runs
                .iter()
                .map(|(id, entry)| entry.message(id, include_children))
                .collect(),
        }
    }

    /// The status of the session `id` and what each source says.
    pub(crate) fn explain(&self, id: &SessionId) -> AgentExplainResponse {
        let mut entries = self.entries();
        Self::read(&mut entries, id).explanation(id)
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
            .map(|id| changed(id, Self::read(&mut entries, id)))
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

    /// Changes the adapter source of the session `id` with `change`, and
    /// resolves its Agent run again. A change of the details alone is a
    /// change too.
    fn change_adapter(
        &self,
        id: &SessionId,
        change: impl FnOnce(&mut Sources) -> Result<(), AdapterReportRefused>,
    ) -> Result<(), AdapterReportRefused> {
        let mut entries = self.entries();
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.holder.is_some() || entry.starting.is_some())
            .ok_or(AdapterReportRefused::NoProcess)?;
        let details_before = entry.sources.details.clone();
        change(&mut entry.sources)?;
        let run = resolve(&entry.reports(), Some(&entry.run), clock::now());
        if run != entry.run || entry.sources.details != details_before {
            entry.run = run;
            self.publish_changed(id, entry);
        }
        Ok(())
    }

    /// The entry of the session `id` if the holder `facts` is its current
    /// holder.
    fn current<'entries>(
        entries: &'entries mut HashMap<SessionId, Entry>,
        id: &SessionId,
        facts: HolderFacts,
    ) -> Option<&'entries mut Entry> {
        entries.get_mut(id).filter(|entry| {
            entry.holder.is_some_and(|known| {
                known.child_pid == facts.child_pid && known.token == facts.token
            })
        })
    }

    fn oldest_kept(entries: &HashMap<SessionId, Entry>) -> Option<SessionId> {
        entries
            .iter()
            .filter_map(|(id, entry)| Some((entry.ended?, id)))
            .min_by_key(|(ended, id)| (*ended, id.as_str()))
            .map(|(_, id)| id.clone())
    }

    fn check_token(entry: &Entry, token: &str) -> Result<(), TokenRefused> {
        let known = match (entry.holder, entry.starting) {
            (Some(holder), _) => holder.token.ok_or(TokenRefused::HolderWithoutToken)?,
            (None, Some(starting)) => starting,
            (None, None) => return Err(TokenRefused::NoProcess),
        };
        if !known.matches(token) {
            return Err(TokenRefused::WrongToken);
        }
        Ok(())
    }

    fn resolve_and_publish(&self, id: &SessionId, entry: &mut Entry) {
        let run = resolve(&entry.reports(), Some(&entry.run), clock::now());
        if run != entry.run {
            entry.run = run;
            self.publish_changed(id, entry);
        }
    }

    fn publish_changed(&self, id: &SessionId, entry: &Entry) {
        self.events.publish(changed(id, entry));
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<SessionId, Entry>> {
        // No code panics while it holds the lock, thus the map is whole.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Entry {
    /// The Agent run of the session `id` as a message.
    fn message(&self, id: &SessionId, include_children: bool) -> nomux_proto::wire::v1::AgentRun {
        let details = self.sources.details.as_ref();
        message::agent_run(id, &self.run, details, include_children)
    }

    fn unreported() -> Self {
        Self {
            run: AgentRun::unreported(clock::now()),
            holder: None,
            starting: None,
            ended: None,
            sources: Sources::default(),
        }
    }

    fn explanation(&self, id: &SessionId) -> AgentExplainResponse {
        AgentExplainResponse {
            run: Some(self.message(id, true)),
            sources: self.reports().iter().map(message::source_report).collect(),
        }
    }

    /// The session has no process now. What stays of its Agent run is the
    /// report of the session restore. The details stay: a resume command
    /// continues the work of the lost process.
    fn keep_what_stays(&mut self) {
        self.holder = None;
        self.starting = None;
        self.sources = Sources {
            carried_over: carried_over(&self.run),
            details: self.sources.details.take(),
            ..Sources::default()
        };
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
            self.sources.adapter.report(),
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

fn changed(id: &SessionId, entry: &Entry) -> event::Body {
    event::Body::AgentRunChanged(Box::new(AgentRunChanged {
        run: Some(entry.message(id, true)),
    }))
}

#[cfg(test)]
mod tests {
    use nomux_holder_protocol::program_reports::{InputLine, ProgramState, ProgramStatus};
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
            Err(TokenRefused::NoProcess)
        );

        registry.connected(&id, holder(None, NO_PROCESS));
        assert_eq!(
            registry.self_report(&id, "", report(State::Working)),
            Err(TokenRefused::HolderWithoutToken)
        );

        registry.connected(&id, holder(Some(token(1)), NO_PROCESS));
        assert_eq!(
            registry.self_report(&id, token(2).as_str(), report(State::Working)),
            Err(TokenRefused::WrongToken)
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
            Err(TokenRefused::WrongToken)
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
            Err(TokenRefused::NoProcess)
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
    fn an_adapter_report_needs_a_process_and_its_details_stay_after_the_process_is_lost() {
        let registry = registry();
        let id = session();
        let working = || RunReport {
            observation: Observation::of(State::Working),
            details: Some(AgentDetails {
                resume_command: "testtool --resume 1".to_owned(),
                ..AgentDetails::default()
            }),
        };
        assert_eq!(
            registry.adapter_run(&id, working()),
            Err(AdapterReportRefused::NoProcess)
        );

        let old = holder(Some(token(1)), NO_PROCESS);
        registry.connected(&id, old);
        registry.adapter_run(&id, working()).unwrap();
        assert_eq!(
            state_of(&registry, &id),
            (AgentState::Working, StatusSource::Adapter)
        );

        registry.lost_process(&id, old);
        let run = registry.explain(&id).run.unwrap();
        assert_eq!(run.status.unwrap().source(), StatusSource::SessionRestore);
        assert_eq!(run.details.unwrap().resume_command, "testtool --resume 1");
        assert_eq!(
            registry.adapter_run(&id, working()),
            Err(AdapterReportRefused::NoProcess),
            "a session without a process has no adapter report"
        );

        // A new process has no report and no details of the old one.
        registry.connected(&id, holder(Some(token(2)), NO_PROCESS - 1));
        let run = registry.explain(&id).run.unwrap();
        assert_eq!(run.details, None);
    }

    #[test]
    fn the_oldest_kept_run_goes_when_more_ended_sessions_keep_one_than_the_limit() {
        let registry = registry();
        let ids: Vec<SessionId> = (0..=MAX_KEPT_RUNS)
            .map(|number| format!("s{number}").parse().unwrap())
            .collect();
        for (number, id) in ids.iter().enumerate() {
            let facts = holder(Some(token(1)), NO_PROCESS - u32::try_from(number).unwrap());
            registry.connected(id, facts);
            registry.program_reports(id, &working(ProgramState::Working));
            registry.ended(id, facts);
        }

        let kept = registry.with_kept_runs(Vec::new());
        assert_eq!(kept.len(), MAX_KEPT_RUNS);
        assert!(!kept.contains(&ids[0]), "the oldest kept run is gone");
        assert!(registry.explain_kept(&ids[0]).is_none());
        let run = registry
            .explain_kept(&ids[MAX_KEPT_RUNS])
            .unwrap()
            .run
            .unwrap();
        assert_eq!(run.status.unwrap().source(), StatusSource::SessionRestore);
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
