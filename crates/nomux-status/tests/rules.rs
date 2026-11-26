//! The rules of the status engine, one named case for each rule.
//! `docs/status.md` has each rule with its reason.

use nomux_status::{
    AgentRun, ChildObservation, MAX_CHILD_WORK, MAX_DEPTH, NeedsYouKind, Observation, Source,
    SourceReport, State, Status, Timestamp, carried_over, most_urgent, resolve,
};

const EARLIER: Timestamp = Timestamp(1_000);
const NOW: Timestamp = Timestamp(5_000);
const LATER: Timestamp = Timestamp(9_000);

fn status(state: State, since: u64, seen: bool) -> Status {
    Status {
        state,
        source: Some(Source::ProgramStatus),
        since: Timestamp(since),
        seen,
        ..Status::unreported(Timestamp(since))
    }
}

fn says(state: State, message: Option<&str>) -> Observation {
    Observation {
        message: message.map(str::to_owned),
        ..Observation::of(state)
    }
}

fn report(source: Source, root: Option<Observation>) -> SourceReport {
    SourceReport {
        source,
        root,
        children: Vec::new(),
        note: String::new(),
    }
}

fn child(path: &[&str], state: State) -> ChildObservation {
    ChildObservation {
        path: path.iter().map(|name| (*name).to_owned()).collect(),
        observation: Observation::of(state),
    }
}

fn with_children(source: Source, children: Vec<ChildObservation>) -> SourceReport {
    SourceReport {
        children,
        ..report(source, None)
    }
}

fn path(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

const PERMISSION: State = State::NeedsYou(Some(NeedsYouKind::Permission));

#[test]
fn the_aggregation_priority_and_the_tie_rule() {
    struct Case {
        name: &'static str,
        candidates: Vec<(&'static str, Status)>,
        winner: &'static str,
    }
    let cases = [
        Case {
            name: "needs you beats an error that the user did not see",
            candidates: vec![
                ("error", status(State::Error, 1, false)),
                ("needs you", status(PERMISSION, 9, false)),
            ],
            winner: "needs you",
        },
        Case {
            name: "an error that the user did not see beats a result that the user did not see",
            candidates: vec![
                ("done", status(State::Done, 1, false)),
                ("error", status(State::Error, 9, false)),
            ],
            winner: "error",
        },
        Case {
            name: "a result that the user did not see beats work that runs",
            candidates: vec![
                ("working", status(State::Working, 1, false)),
                ("done", status(State::Done, 9, false)),
            ],
            winner: "done",
        },
        Case {
            name: "work that runs beats idle work",
            candidates: vec![
                ("idle", status(State::Idle, 1, false)),
                ("working", status(State::Working, 9, false)),
            ],
            winner: "working",
        },
        Case {
            name: "a result that the user saw is quiet: work that runs beats it",
            candidates: vec![
                ("seen error", status(State::Error, 1, true)),
                ("seen done", status(State::Done, 1, true)),
                ("working", status(State::Working, 9, false)),
            ],
            winner: "working",
        },
        Case {
            name: "with the same priority, the oldest since wins",
            candidates: vec![
                ("newer", status(PERMISSION, 9, false)),
                ("older", status(State::NeedsYou(None), 1, false)),
            ],
            winner: "older",
        },
        Case {
            name: "a quiet result and idle work have the same priority: the oldest wins",
            candidates: vec![
                ("idle", status(State::Idle, 9, false)),
                ("seen done", status(State::Done, 1, true)),
            ],
            winner: "seen done",
        },
        Case {
            name: "with the same priority and the same since, the first wins",
            candidates: vec![
                ("first", status(State::Working, 5, false)),
                ("second", status(State::Working, 5, false)),
            ],
            winner: "first",
        },
    ];
    for case in cases {
        let winner = most_urgent(case.candidates.iter().map(|(key, status)| (*key, status)));
        assert_eq!(
            winner.map(|(key, _)| key),
            Some(case.winner),
            "{}",
            case.name
        );
    }
}

#[test]
fn the_sources_decide_in_their_fixed_sequence() {
    struct Case {
        name: &'static str,
        reports: Vec<SourceReport>,
        state: State,
        source: Option<Source>,
    }
    let cases = [
        Case {
            name: "the program status decides before the self-report, in each sequence",
            reports: vec![
                report(Source::SelfReport, Some(says(State::Done, None))),
                report(Source::ProgramStatus, Some(says(State::Working, None))),
            ],
            state: State::Working,
            source: Some(Source::ProgramStatus),
        },
        Case {
            name: "the self-report decides when the program status says nothing",
            reports: vec![
                report(Source::ProgramStatus, None),
                report(Source::SelfReport, Some(says(State::Error, None))),
                report(Source::PromptMarks, Some(says(State::Working, None))),
            ],
            state: State::Error,
            source: Some(Source::SelfReport),
        },
        Case {
            name: "a report of a program decides before a guess",
            reports: vec![
                report(Source::ForegroundProgram, Some(says(State::Working, None))),
                report(Source::SessionRestore, Some(says(State::Done, None))),
            ],
            state: State::Done,
            source: Some(Source::SessionRestore),
        },
        Case {
            name: "the prompt marks decide before the foreground program",
            reports: vec![
                report(Source::ForegroundProgram, Some(says(State::Working, None))),
                report(Source::PromptMarks, Some(says(State::Idle, None))),
            ],
            state: State::Idle,
            source: Some(Source::PromptMarks),
        },
        Case {
            name: "without a report, the work is idle and has no source",
            reports: vec![report(Source::ProgramStatus, None)],
            state: State::Idle,
            source: None,
        },
    ];
    for case in cases {
        let run = resolve(&case.reports, None, NOW);
        assert_eq!(
            (run.status.state, run.status.source),
            (case.state, case.source),
            "{}",
            case.name
        );
    }
}

#[test]
fn a_text_comes_only_from_a_source_that_reports_the_same_state() {
    let reports = [
        report(Source::ProgramStatus, Some(says(State::Working, None))),
        report(
            Source::SelfReport,
            Some(says(State::Done, Some("tests passed"))),
        ),
        report(
            Source::PromptMarks,
            Some(Observation {
                title: Some("build".to_owned()),
                ..says(State::Working, Some("a command runs"))
            }),
        ),
    ];

    let run = resolve(&reports, None, NOW);

    // "tests passed" describes a different state.
    let message = run.status.message.unwrap();
    assert_eq!(
        (message.value.as_str(), message.source),
        ("a command runs", Source::PromptMarks)
    );
    // A label does not describe a state: it comes from the first source
    // that has one.
    assert_eq!(run.status.title.unwrap().source, Source::PromptMarks);
}

#[test]
fn since_and_seen_stay_while_the_state_stays() {
    let working = |message| {
        [report(
            Source::ProgramStatus,
            Some(says(State::Working, message)),
        )]
    };
    let first = resolve(&working(Some("step 1")), None, EARLIER);
    assert_eq!(first.status.since, EARLIER);

    // A new message is not a new state.
    let second = resolve(&working(Some("step 2")), Some(&first), NOW);
    assert_eq!(second.status.since, EARLIER);

    // A new state starts a new time, and its result is not seen.
    let done = [report(Source::ProgramStatus, Some(says(State::Done, None)))];
    let mut third = resolve(&done, Some(&second), NOW);
    assert_eq!((third.status.since, third.status.seen), (NOW, false));

    assert!(third.mark_seen());
    assert!(!third.mark_seen(), "a second look changes nothing");
    let fourth = resolve(&done, Some(&third), LATER);
    assert_eq!((fourth.status.since, fourth.status.seen), (NOW, true));

    // A different kind of wait is a new state.
    let ask = |kind| {
        [report(
            Source::ProgramStatus,
            Some(says(State::NeedsYou(Some(kind)), None)),
        )]
    };
    let permission = resolve(&ask(NeedsYouKind::Permission), None, EARLIER);
    let question = resolve(&ask(NeedsYouKind::Question), Some(&permission), NOW);
    assert_eq!(question.status.since, NOW);
}

#[test]
fn work_that_no_source_reports_any_more_is_idle_from_then_on() {
    let working = [report(
        Source::ProgramStatus,
        Some(says(State::Working, None)),
    )];
    let silent = [report(Source::ProgramStatus, None)];

    let run = resolve(&working, None, EARLIER);
    let gone = resolve(&silent, Some(&run), NOW);
    assert_eq!((gone.status.state, gone.status.source), (State::Idle, None));
    assert_eq!(gone.status.since, NOW);

    // Idle work without a source keeps its time.
    let still = resolve(&silent, Some(&gone), LATER);
    assert_eq!(still.status.since, NOW);
}

#[test]
fn child_work_is_a_tree_and_its_parent_aggregates_it() {
    let reports = [with_children(
        Source::ProgramStatus,
        vec![
            child(&["build", "test"], PERMISSION),
            child(&["lint"], State::Working),
        ],
    )];

    let run = resolve(&reports, None, NOW);

    assert_eq!(run.status.state, State::Idle);
    assert_eq!(run.aggregate.state, PERMISSION, "a child that needs you");
    let names: Vec<&str> = run
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    assert_eq!(names, ["build", "lint"]);
    let build = &run.children[0];
    assert_eq!(build.status, None, "only its child reports");
    assert_eq!(build.aggregate.state, PERMISSION);
    assert_eq!(
        run.child_status(&path(&["build", "test"])).unwrap().state,
        PERMISSION
    );
}

#[test]
fn the_tree_has_a_limit_of_depth_and_of_size() {
    let deepest: Vec<String> = (0..MAX_DEPTH).map(|level| format!("l{level}")).collect();
    let too_deep: Vec<String> = (0..=MAX_DEPTH).map(|level| format!("l{level}")).collect();
    let mut children = vec![
        ChildObservation {
            path: deepest.clone(),
            observation: Observation::of(State::Working),
        },
        ChildObservation {
            path: too_deep.clone(),
            observation: Observation::of(PERMISSION),
        },
        ChildObservation {
            path: Vec::new(),
            observation: Observation::of(PERMISSION),
        },
    ];
    children.extend((0..2 * MAX_CHILD_WORK).map(|index| ChildObservation {
        path: vec![format!("task{index:03}")],
        observation: Observation::of(State::Working),
    }));

    let run = resolve(&[with_children(Source::ProgramStatus, children)], None, NOW);

    assert!(
        run.child_status(&deepest).is_some(),
        "8 levels are in the tree"
    );
    assert!(run.child_status(&too_deep).is_none(), "a 9th level is not");
    assert_eq!(
        run.aggregate.state,
        State::Working,
        "nothing ignored decides"
    );
    let mut count = 0;
    count_statuses(&run.children, &mut count);
    assert_eq!(count, MAX_CHILD_WORK);
}

fn count_statuses(children: &[nomux_status::ChildWork], count: &mut usize) {
    for child in children {
        *count += usize::from(child.status.is_some());
        count_statuses(&child.children, count);
    }
}

#[test]
fn a_session_that_lost_its_process_shows_no_running_or_waiting_work() {
    struct Case {
        name: &'static str,
        before: State,
        after: Option<State>,
    }
    let cases = [
        Case {
            name: "work that ran is done",
            before: State::Working,
            after: Some(State::Done),
        },
        Case {
            name: "work that waited for the user is done",
            before: PERMISSION,
            after: Some(State::Done),
        },
        Case {
            name: "a result that the user did not see stays",
            before: State::Done,
            after: Some(State::Done),
        },
        Case {
            name: "a failure stays",
            before: State::Error,
            after: Some(State::Error),
        },
        Case {
            name: "idle work leaves nothing",
            before: State::Idle,
            after: None,
        },
    ];
    for case in cases {
        let reports = [report(
            Source::ProgramStatus,
            Some(says(case.before, Some("message"))),
        )];
        let run = resolve(&reports, None, EARLIER);

        let carried = carried_over(&run);
        let restored = resolve(carried.as_slice(), Some(&run), NOW);

        match case.after {
            Some(state) => {
                assert_eq!(restored.status.state, state, "{}", case.name);
                assert_eq!(
                    restored.status.source,
                    Some(Source::SessionRestore),
                    "{}",
                    case.name
                );
            }
            None => assert!(carried.is_none(), "{}", case.name),
        }
    }
}

#[test]
fn a_restored_result_keeps_its_time_and_a_finished_run_loses_its_live_message() {
    let done = [report(
        Source::ProgramStatus,
        Some(says(State::Done, Some("built"))),
    )];
    let mut finished = resolve(&done, None, EARLIER);
    finished.mark_seen();
    let restored = resolve(carried_over(&finished).as_slice(), Some(&finished), NOW);
    assert_eq!(
        (restored.status.since, restored.status.seen),
        (EARLIER, true)
    );
    assert_eq!(restored.status.message.unwrap().value, "built");

    let working = [report(
        Source::ProgramStatus,
        Some(says(State::Working, Some("reading"))),
    )];
    let running = resolve(&working, None, EARLIER);
    let stopped = resolve(carried_over(&running).as_slice(), Some(&running), NOW);
    assert_eq!(
        stopped.status.message, None,
        "the message described work that ran"
    );
    assert_eq!(stopped.status.since, NOW);
}

#[test]
fn a_guess_does_not_stay_after_the_process_is_gone() {
    let guess = [report(
        Source::ForegroundProgram,
        Some(says(State::Working, None)),
    )];
    let run = resolve(&guess, None, EARLIER);
    assert!(carried_over(&run).is_none());

    let children = [with_children(
        Source::ProgramStatus,
        vec![child(&["agent", "subagent"], State::Working)],
    )];
    let run = resolve(&children, None, EARLIER);
    let restored = resolve(carried_over(&run).as_slice(), Some(&run), NOW);
    let subagent = restored
        .child_status(&path(&["agent", "subagent"]))
        .unwrap();
    assert_eq!(subagent.state, State::Done);
    assert_eq!(
        restored.status.state,
        State::Idle,
        "the run itself had no report"
    );
}

#[test]
fn an_unreported_run_is_idle_and_not_a_result() {
    let mut run = AgentRun::unreported(NOW);
    assert_eq!(run.aggregate.state, State::Idle);
    assert!(!run.mark_seen(), "idle work is not a result");
}
