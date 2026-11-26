//! The program reports of a holder as reports of two status sources: the
//! program status (OSC 7501) and the prompt marks (OSC 133).

use nomux_holder_protocol::program_reports::{
    BlockedKind, InputLine, ProgramReports, ProgramState, ProgramStatus, PromptMark,
};
use nomux_status::{ChildObservation, NeedsYouKind, Observation, Source, SourceReport, State};

/// The largest number of bytes of a message in a status. A program can
/// report a message of 2048 bytes for each of its 64 records, and one
/// response has the status of each session.
pub(crate) const MAX_MESSAGE_LENGTH: usize = 512;

/// What the programs of the session reported with the Program Status
/// Protocol.
pub(crate) fn program_status(reports: &ProgramReports) -> SourceReport {
    let current: Vec<&ProgramStatus> = reports
        .statuses
        .iter()
        .filter(|status| !is_left_by_ended_program(status, &reports.input_line))
        .collect();
    let left = reports.statuses.len() - current.len();
    let mut report = SourceReport {
        source: Source::ProgramStatus,
        root: None,
        children: Vec::new(),
        note: note_of_records(current.len(), left),
    };
    for status in current {
        let observation = observation(status);
        if status.id.is_empty() {
            report.root = Some(observation);
        } else {
            report.children.push(ChildObservation {
                path: status.id.split('/').map(str::to_owned).collect(),
                observation,
            });
        }
    }
    report
}

/// What the prompt marks of the shell say. This is a guess: the marks say
/// that a command runs, not what it does.
pub(crate) fn prompt_marks(line: &InputLine) -> SourceReport {
    let (state, note) = match line.mark {
        None => {
            return SourceReport::silent(
                Source::PromptMarks,
                "the shell of the session sent no prompt mark",
            );
        }
        Some(PromptMark::CommandStart) => (
            State::Working,
            "the shell marked the start of a command, and no end",
        ),
        Some(PromptMark::CommandEnd) => (State::Idle, "the shell marked the end of a command"),
        Some(PromptMark::PromptStart | PromptMark::InputStart) => (
            State::Idle,
            "the shell shows its prompt: the input line waits",
        ),
    };
    SourceReport {
        source: Source::PromptMarks,
        root: Some(Observation::of(state)),
        children: Vec::new(),
        note: note.to_owned(),
    }
}

/// The rule of the Program Status Protocol for the life of a record: a new
/// prompt removes the records of work that runs or waits. The program that
/// reported them ended without a last report. The holder keeps the records,
/// thus the daemon applies the rule: the record is older than the prompt
/// that the shell shows now.
fn is_left_by_ended_program(status: &ProgramStatus, line: &InputLine) -> bool {
    let at_prompt = matches!(
        line.mark,
        Some(PromptMark::PromptStart | PromptMark::InputStart)
    );
    let needs_program = matches!(status.state, ProgramState::Working | ProgramState::Blocked);
    at_prompt && needs_program && status.revision < line.revision
}

fn observation(status: &ProgramStatus) -> Observation {
    let state = match status.state {
        ProgramState::Idle => State::Idle,
        ProgramState::Working => State::Working,
        ProgramState::Done => State::Done,
        ProgramState::Blocked => State::NeedsYou(status.kind.map(needs_you_kind)),
        ProgramState::Error => State::Error,
    };
    Observation {
        state,
        message: status.message.as_deref().map(shortened),
        title: status.title.clone(),
        progress: status.progress,
        tool: status.app.clone(),
        soft: false,
    }
}

fn needs_you_kind(kind: BlockedKind) -> NeedsYouKind {
    match kind {
        BlockedKind::Permission => NeedsYouKind::Permission,
        BlockedKind::Question => NeedsYouKind::Question,
        BlockedKind::Auth => NeedsYouKind::Auth,
    }
}

/// The start of `message`: [`MAX_MESSAGE_LENGTH`] bytes at most, and no
/// part of a character.
fn shortened(message: &str) -> String {
    let mut end = message.len().min(MAX_MESSAGE_LENGTH);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}

fn note_of_records(current: usize, left: usize) -> String {
    match (current, left) {
        (0, 0) => "no program of the session reported a status".to_owned(),
        (current, 0) => format!("{current} program status records in the terminal stream"),
        (current, left) => format!(
            "{current} program status records in the terminal stream; {left} more are older \
             than the prompt of the shell: their program ended"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, state: ProgramState, revision: u64) -> ProgramStatus {
        ProgramStatus {
            id: id.to_owned(),
            state,
            kind: None,
            progress: None,
            app: None,
            title: None,
            message: None,
            revision,
        }
    }

    fn line(mark: PromptMark, revision: u64) -> InputLine {
        InputLine {
            mark: Some(mark),
            exit_code: None,
            input_after_mark: false,
            revision,
        }
    }

    fn reports(statuses: Vec<ProgramStatus>, input_line: InputLine) -> ProgramReports {
        ProgramReports {
            revision: 9,
            input_line,
            statuses,
            notifications: Vec::new(),
        }
    }

    #[test]
    fn a_record_becomes_the_agent_run_or_a_child_work_by_its_id() {
        let blocked = ProgramStatus {
            kind: Some(BlockedKind::Permission),
            message: Some("run tests?".to_owned()),
            app: Some("codex".to_owned()),
            ..record("", ProgramState::Blocked, 1)
        };
        let child = record("build/test", ProgramState::Working, 2);

        let report = program_status(&reports(vec![blocked, child], InputLine::default()));

        let root = report.root.unwrap();
        assert_eq!(root.state, State::NeedsYou(Some(NeedsYouKind::Permission)));
        assert_eq!(root.message.as_deref(), Some("run tests?"));
        assert_eq!(root.tool.as_deref(), Some("codex"));
        assert_eq!(report.children.len(), 1);
        assert_eq!(report.children[0].path, ["build", "test"]);
        assert_eq!(report.children[0].observation.state, State::Working);
    }

    #[test]
    fn a_new_prompt_removes_only_the_records_of_work_that_needs_its_program() {
        let statuses = vec![
            record("", ProgramState::Working, 3),
            record("ask", ProgramState::Blocked, 4),
            record("result", ProgramState::Done, 5),
            record("late", ProgramState::Working, 8),
        ];

        let at_prompt = program_status(&reports(statuses.clone(), line(PromptMark::InputStart, 7)));
        let paths: Vec<&[String]> = at_prompt
            .children
            .iter()
            .map(|child| child.path.as_slice())
            .collect();
        assert_eq!(at_prompt.root, None);
        assert_eq!(paths, [["result"], ["late"]]);

        // While a command runs, the shell has no new prompt: each record
        // stays, also when input went to the terminal after the report.
        let running = program_status(&reports(statuses, line(PromptMark::CommandStart, 7)));
        assert!(running.root.is_some());
        assert_eq!(running.children.len(), 3);
    }

    #[test]
    fn a_long_message_is_cut_at_a_character() {
        let message = "é".repeat(MAX_MESSAGE_LENGTH);
        let status = ProgramStatus {
            message: Some(message),
            ..record("", ProgramState::Working, 1)
        };

        let report = program_status(&reports(vec![status], InputLine::default()));

        let message = report.root.unwrap().message.unwrap();
        assert_eq!(message, "é".repeat(MAX_MESSAGE_LENGTH / 2));
    }

    #[test]
    fn the_prompt_marks_guess_only_from_a_mark() {
        let state = |mark| prompt_marks(&line(mark, 1)).root.map(|root| root.state);

        assert_eq!(prompt_marks(&InputLine::default()).root, None);
        assert_eq!(state(PromptMark::CommandStart), Some(State::Working));
        assert_eq!(state(PromptMark::CommandEnd), Some(State::Idle));
        assert_eq!(state(PromptMark::PromptStart), Some(State::Idle));
        assert_eq!(state(PromptMark::InputStart), Some(State::Idle));
    }
}
