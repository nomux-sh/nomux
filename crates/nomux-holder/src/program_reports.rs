//! The program reports that the holder keeps: the last state from the
//! records that `nomux-term` parsed from the terminal output.
//!
//! This module is the one place that changes the parsed records of
//! `nomux-term` into the wire types of `nomux-holder-protocol`. The holder
//! keeps the records and reads no meaning into them: it does not remove a
//! record when a prompt starts or when the command ends. The daemon does
//! that with the revisions.

mod status_records;

use nomux_holder_protocol::program_reports::{
    BlockedKind, InputLine, Notification, ProgramReports, ProgramState, ProgramStatus, PromptMark,
};
use nomux_term::osc;

/// The largest number of program status records. The specification of
/// OSC 7501 asks for 64 or more. When a report makes one more record, the
/// record with the oldest report is removed.
const MAX_STATUSES: usize = 64;

/// The largest number of notifications. A new notification removes the
/// oldest one.
const MAX_NOTIFICATIONS: usize = 16;

/// The memory of the store has a limit, because each text of a record has a
/// limit in its parser: 64 records of 2400 bytes and 16 notifications of
/// 2240 bytes, approximately 190 KiB.
#[derive(Default)]
pub(crate) struct ProgramReportStore {
    reports: ProgramReports,
}

impl ProgramReportStore {
    /// The number of changes to the store.
    pub(crate) fn revision(&self) -> u64 {
        self.reports.revision
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        self.reports.encode(&mut encoded);
        encoded
    }

    /// Does one program status report. `app_agrees` says if the foreground
    /// program of the terminal agrees with an `app` value: a report that
    /// claims a different program is ignored.
    pub(crate) fn report_status(
        &mut self,
        report: osc::ProgramStatusReport,
        app_agrees: impl FnOnce(&str) -> bool,
    ) {
        let statuses = &mut self.reports.statuses;
        let claimed_app = status_records::claimed_app(statuses, &report);
        if claimed_app.is_some_and(|app| !app_agrees(app)) {
            return;
        }
        let Some(state) = wire_state(report.state) else {
            if status_records::clear(statuses, &report.id) {
                self.reports.revision += 1;
            }
            return;
        };
        self.reports.revision += 1;
        let record = ProgramStatus {
            id: report.id,
            state,
            kind: report.kind.map(wire_kind),
            progress: report.progress,
            app: report.app,
            title: report.title,
            message: report.message,
            revision: self.reports.revision,
        };
        status_records::replace(statuses, record, MAX_STATUSES);
    }

    pub(crate) fn mark_prompt(&mut self, mark: osc::PromptMark) {
        let last_exit_code = self.reports.input_line.exit_code;
        let (mark, exit_code) = match mark {
            osc::PromptMark::PromptStart => (PromptMark::PromptStart, last_exit_code),
            osc::PromptMark::InputStart => (PromptMark::InputStart, last_exit_code),
            osc::PromptMark::CommandStart => (PromptMark::CommandStart, last_exit_code),
            osc::PromptMark::CommandEnd { exit_code } => (PromptMark::CommandEnd, exit_code),
        };
        self.reports.revision += 1;
        self.reports.input_line = InputLine {
            mark: Some(mark),
            exit_code,
            input_after_mark: false,
            revision: self.reports.revision,
        };
    }

    pub(crate) fn notify(&mut self, notification: osc::Notification) {
        self.reports.revision += 1;
        let notifications = &mut self.reports.notifications;
        if notifications.len() == MAX_NOTIFICATIONS {
            notifications.remove(0);
        }
        notifications.push(Notification {
            title: notification.title,
            body: notification.body,
            revision: self.reports.revision,
        });
    }

    /// Records that the holder wrote input to the terminal. Without a
    /// prompt mark, the input line state is not known and does not change.
    pub(crate) fn input_written(&mut self) {
        let line = &mut self.reports.input_line;
        if line.mark.is_none() || line.input_after_mark {
            return;
        }
        self.reports.revision += 1;
        line.input_after_mark = true;
        line.revision = self.reports.revision;
    }
}

/// `None` for `clear`, which is not a state.
fn wire_state(state: osc::ReportedState) -> Option<ProgramState> {
    match state {
        osc::ReportedState::Idle => Some(ProgramState::Idle),
        osc::ReportedState::Working => Some(ProgramState::Working),
        osc::ReportedState::Done => Some(ProgramState::Done),
        osc::ReportedState::Blocked => Some(ProgramState::Blocked),
        osc::ReportedState::Error => Some(ProgramState::Error),
        osc::ReportedState::Clear => None,
    }
}

fn wire_kind(kind: osc::BlockedKind) -> BlockedKind {
    match kind {
        osc::BlockedKind::Permission => BlockedKind::Permission,
        osc::BlockedKind::Question => BlockedKind::Question,
        osc::BlockedKind::Auth => BlockedKind::Auth,
    }
}

#[cfg(test)]
mod tests {
    use nomux_holder_protocol::program_reports::{ProgramState, PromptMark};
    use nomux_term::osc::{self, OscRecord};

    use super::{MAX_NOTIFICATIONS, MAX_STATUSES, ProgramReportStore};

    /// The foreground program of these tests has the name `tool`.
    fn report(store: &mut ProgramReportStore, payload: &str) {
        let osc = osc::Osc {
            code: 7501,
            payload: payload.as_bytes(),
        };
        let Some(OscRecord::ProgramStatus(report)) = osc.record() else {
            panic!("not a program status report: {payload}");
        };
        store.report_status(report, |app| app == "tool");
    }

    fn ids(store: &ProgramReportStore) -> Vec<&str> {
        let statuses = &store.reports.statuses;
        statuses.iter().map(|status| status.id.as_str()).collect()
    }

    #[test]
    fn a_report_that_claims_a_different_program_changes_nothing() {
        let mut store = ProgramReportStore::default();
        report(&mut store, "state=working:app=tool");
        let before = store.reports.clone();

        report(&mut store, "state=blocked:kind=permission:app=claude-code");
        report(&mut store, "state=blocked:app=claude-code:id=agent");
        report(&mut store, "state=clear:app=claude-code");

        assert_eq!(store.reports, before);
    }

    #[test]
    fn a_report_without_an_app_claims_the_app_of_the_nearest_record_above_it() {
        let mut store = ProgramReportStore::default();
        report(&mut store, "state=working");
        report(&mut store, "state=working:id=free");
        assert_eq!(ids(&store), ["", "free"]);

        // The root record now has the name of a program that is not in the
        // foreground. Each record below it gets that name.
        store.report_status(
            osc::ProgramStatusReport {
                app: Some("claude-code".to_owned()),
                ..status_report("")
            },
            |_| true,
        );
        let before = store.reports.clone();
        report(&mut store, "state=blocked:id=agent/task");
        report(&mut store, "state=clear:id=free");
        assert_eq!(store.reports, before);

        // A record between them with the name of the foreground program is
        // nearer than the root record.
        report(&mut store, "state=working:app=tool:id=agent");
        report(&mut store, "state=blocked:id=agent/task");
        assert_eq!(ids(&store), ["", "free", "agent", "agent/task"]);
    }

    fn status_report(id: &str) -> osc::ProgramStatusReport {
        osc::ProgramStatusReport {
            id: id.to_owned(),
            state: osc::ReportedState::Idle,
            kind: None,
            progress: None,
            app: None,
            title: None,
            message: None,
        }
    }

    #[test]
    fn a_report_replaces_all_the_keys_of_its_record() {
        let mut store = ProgramReportStore::default();
        report(
            &mut store,
            "state=blocked:kind=auth:progress=5:app=tool:title=QQ==:msg=QQ==",
        );

        report(&mut store, "state=done");

        let root = &store.reports.statuses[0];
        assert_eq!(root.state, ProgramState::Done);
        assert_eq!((root.kind, root.progress), (None, None));
        assert_eq!(
            (&root.app, &root.title, &root.message),
            (&None, &None, &None)
        );
        assert_eq!(root.revision, 2);
        assert_eq!(store.reports.statuses.len(), 1);
    }

    #[test]
    fn clear_removes_the_record_and_the_records_below_it_only() {
        let mut store = ProgramReportStore::default();
        for id in [
            "build",
            "build/test",
            "build/test/unit",
            "builder",
            "deploy",
        ] {
            report(&mut store, &format!("state=working:id={id}"));
        }
        report(&mut store, "state=working");

        report(&mut store, "state=clear:id=build");
        assert_eq!(ids(&store), ["builder", "deploy", ""]);
        assert_eq!(store.revision(), 7);

        // A clear that removes nothing is not a change.
        report(&mut store, "state=clear:id=build");
        assert_eq!(store.revision(), 7);

        report(&mut store, "state=clear");
        assert_eq!(ids(&store), [""; 0]);
        assert_eq!(store.revision(), 8);
    }

    #[test]
    fn the_record_with_the_oldest_report_makes_room_for_a_new_record() {
        let mut store = ProgramReportStore::default();
        for index in 0..MAX_STATUSES {
            report(&mut store, &format!("state=working:id=task-{index}"));
        }
        report(&mut store, "state=done:id=task-0");

        report(&mut store, "state=working:id=one-more");

        let ids = ids(&store);
        assert_eq!(ids.len(), MAX_STATUSES);
        assert!(ids.contains(&"task-0"));
        assert!(!ids.contains(&"task-1"));
        assert!(ids.contains(&"one-more"));
    }

    #[test]
    fn the_oldest_notification_makes_room_for_a_new_notification() {
        let mut store = ProgramReportStore::default();
        for index in 0..=MAX_NOTIFICATIONS {
            store.notify(osc::Notification {
                title: None,
                body: format!("note {index}"),
            });
        }

        let notifications = &store.reports.notifications;
        assert_eq!(notifications.len(), MAX_NOTIFICATIONS);
        assert_eq!(notifications[0].body, "note 1");
        assert_eq!(notifications[MAX_NOTIFICATIONS - 1].body, "note 16");
        assert_eq!(notifications[MAX_NOTIFICATIONS - 1].revision, 17);
    }

    #[test]
    fn input_changes_the_input_line_state_one_time_after_each_mark() {
        let mut store = ProgramReportStore::default();
        store.input_written();
        assert_eq!(store.revision(), 0, "no mark: the state is not known");

        store.mark_prompt(osc::PromptMark::InputStart);
        store.input_written();
        store.input_written();
        assert!(store.reports.input_line.input_after_mark);
        assert_eq!(store.revision(), 2);
        assert_eq!(store.reports.input_line.revision, 2);

        store.mark_prompt(osc::PromptMark::CommandStart);
        assert!(!store.reports.input_line.input_after_mark);
        assert_eq!(
            store.reports.input_line.mark,
            Some(PromptMark::CommandStart)
        );
    }

    #[test]
    fn the_exit_code_stays_until_the_next_command_ends() {
        let mut store = ProgramReportStore::default();
        store.mark_prompt(osc::PromptMark::CommandEnd { exit_code: Some(3) });
        store.mark_prompt(osc::PromptMark::PromptStart);
        store.mark_prompt(osc::PromptMark::InputStart);
        store.mark_prompt(osc::PromptMark::CommandStart);
        assert_eq!(store.reports.input_line.exit_code, Some(3));

        store.mark_prompt(osc::PromptMark::CommandEnd { exit_code: None });
        assert_eq!(store.reports.input_line.exit_code, None);
        assert_eq!(store.reports.input_line.mark, Some(PromptMark::CommandEnd));
    }
}
