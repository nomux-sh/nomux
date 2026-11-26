//! The rules of the adapter source: the merge table of an adapter report and
//! a later source, the rules of a soft working report, and the tree of the
//! Child work of an adapter. `docs/status.md` has the tables.

use nomux_status::{
    AdapterRefusal, AdapterSource, ChildChange, MAX_CHILD_WORK, MAX_DEPTH, NeedsYouKind,
    Observation, Source, SourceReport, State, Timestamp, resolve,
};

const NOW: Timestamp = Timestamp(5_000);
const PERMISSION: State = State::NeedsYou(Some(NeedsYouKind::Permission));

fn firm(state: State) -> Observation {
    Observation::of(state)
}

fn soft_working() -> Observation {
    Observation {
        soft: true,
        tool: Some("edit".to_owned()),
        ..Observation::of(State::Working)
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

fn path(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn start(source: &mut AdapterSource, id: &str, parent: Option<&str>) -> Result<(), AdapterRefusal> {
    let change = ChildChange::Start {
        parent: parent.map(str::to_owned),
    };
    source.report_child(id, change, firm(State::Working))
}

/// The merge table of `docs/status.md`: an adapter report against the
/// report of a later source for the same item.
#[test]
fn the_merge_table_of_the_adapter_and_a_later_source() {
    struct Row {
        name: &'static str,
        adapter: Option<Observation>,
        later: Option<State>,
        state: State,
        source: Source,
    }
    let rows = [
        Row {
            name: "a firm adapter state decides before a later state that needs the user",
            adapter: Some(firm(State::Working)),
            later: Some(PERMISSION),
            state: State::Working,
            source: Source::Adapter,
        },
        Row {
            name: "a firm adapter result decides before a later working state",
            adapter: Some(firm(State::Done)),
            later: Some(State::Working),
            state: State::Done,
            source: Source::Adapter,
        },
        Row {
            name: "a soft working state yields to a later state that needs the user",
            adapter: Some(soft_working()),
            later: Some(PERMISSION),
            state: PERMISSION,
            source: Source::ProgramStatus,
        },
        Row {
            name: "a soft working state decides before a later result",
            adapter: Some(soft_working()),
            later: Some(State::Done),
            state: State::Working,
            source: Source::Adapter,
        },
        Row {
            name: "a soft working state decides without a later source",
            adapter: Some(soft_working()),
            later: None,
            state: State::Working,
            source: Source::Adapter,
        },
        Row {
            name: "without an adapter report the later source decides",
            adapter: None,
            later: Some(State::Error),
            state: State::Error,
            source: Source::ProgramStatus,
        },
    ];
    for row in rows {
        let reports = [
            report(Source::ProgramStatus, row.later.map(Observation::of)),
            report(Source::Adapter, row.adapter),
        ];
        let status = resolve(&reports, None, NOW).status;
        assert_eq!(
            (status.state, status.source),
            (row.state, Some(row.source)),
            "{}",
            row.name
        );
    }
}

#[test]
fn a_soft_working_state_that_yields_keeps_its_tool_and_not_its_message() {
    let adapter = Observation {
        message: Some("edits a file".to_owned()),
        ..soft_working()
    };
    let program = Observation {
        message: Some("allow the edit?".to_owned()),
        ..Observation::of(PERMISSION)
    };
    let reports = [
        report(Source::Adapter, Some(adapter)),
        report(Source::ProgramStatus, Some(program)),
    ];

    let status = resolve(&reports, None, NOW).status;

    let tool = status.tool.unwrap();
    assert_eq!(
        (tool.value.as_str(), tool.source),
        ("edit", Source::Adapter)
    );
    assert_eq!(status.message.unwrap().value, "allow the edit?");
}

#[test]
fn a_soft_working_child_work_yields_to_a_later_state_that_needs_the_user() {
    let mut adapter = AdapterSource::default();
    start(&mut adapter, "a1", None).unwrap();
    adapter
        .report_child("a1", ChildChange::Update, soft_working())
        .unwrap();
    let program = SourceReport {
        children: vec![nomux_status::ChildObservation {
            path: path(&["a1"]),
            observation: Observation::of(PERMISSION),
        }],
        ..report(Source::ProgramStatus, None)
    };

    let run = resolve(&[adapter.report(), program], None, NOW);

    let child = run.child_status(&path(&["a1"])).unwrap();
    assert_eq!(
        (child.state, child.source),
        (PERMISSION, Some(Source::ProgramStatus))
    );
}

/// The rules of a soft working report in the adapter source itself.
#[test]
fn a_soft_working_report_does_not_replace_a_wait_or_work_in_the_background() {
    struct Row {
        name: &'static str,
        before: State,
        child_works: bool,
        after: State,
    }
    let rows = [
        Row {
            name: "a wait for the user stays",
            before: PERMISSION,
            child_works: false,
            after: PERMISSION,
        },
        Row {
            name: "complete work with working Child work is work in the background and stays",
            before: State::Done,
            child_works: true,
            after: State::Done,
        },
        Row {
            name: "complete work without working Child work works again",
            before: State::Done,
            child_works: false,
            after: State::Working,
        },
        Row {
            name: "a failure works again",
            before: State::Error,
            child_works: false,
            after: State::Working,
        },
    ];
    for row in rows {
        let mut adapter = AdapterSource::default();
        adapter.report_run(firm(row.before));
        if row.child_works {
            start(&mut adapter, "a1", None).unwrap();
        }

        adapter.report_run(soft_working());

        let state = adapter.report().root.map(|root| root.state);
        assert_eq!(state, Some(row.after), "{}", row.name);
    }
}

#[test]
fn a_firm_report_replaces_a_wait_and_idle_takes_the_report_back() {
    let mut adapter = AdapterSource::default();
    adapter.report_run(firm(PERMISSION));

    adapter.report_run(firm(State::Working));
    assert_eq!(adapter.report().root.unwrap().state, State::Working);

    adapter.report_run(firm(State::Idle));
    assert_eq!(adapter.report().root, None);
}

#[test]
fn a_child_work_has_the_path_of_its_parents_and_eight_levels_at_most() {
    let mut adapter = AdapterSource::default();
    let ids: Vec<String> = (1..=MAX_DEPTH).map(|level| format!("c{level}")).collect();
    let mut parent = None;
    for id in &ids {
        start(&mut adapter, id, parent).unwrap();
        parent = Some(id.as_str());
    }

    assert_eq!(
        start(&mut adapter, "too-deep", parent),
        Err(AdapterRefusal::TooDeep)
    );
    let report = adapter.report();
    let deepest = report.children.last().unwrap();
    assert_eq!(deepest.path, ids);
    let run = resolve(&[report], None, NOW);
    assert!(run.child_status(&ids).is_some());
}

#[test]
fn an_ended_child_work_gives_its_place_to_a_new_one_at_the_limit() {
    let mut adapter = AdapterSource::default();
    for index in 0..MAX_CHILD_WORK {
        start(&mut adapter, &format!("c{index}"), None).unwrap();
    }
    assert_eq!(
        start(&mut adapter, "new", None),
        Err(AdapterRefusal::TooMany),
        "no Child work ended"
    );

    adapter
        .report_child("c3", ChildChange::End, firm(State::Done))
        .unwrap();
    start(&mut adapter, "new", None).unwrap();

    let names: Vec<String> = adapter
        .report()
        .children
        .iter()
        .map(|child| child.path.join("/"))
        .collect();
    assert_eq!(names.len(), MAX_CHILD_WORK);
    assert!(!names.contains(&"c3".to_owned()));
    assert_eq!(names.last().unwrap(), "new");
}

#[test]
fn a_child_work_report_must_fit_the_tree_and_the_change() {
    let mut adapter = AdapterSource::default();

    assert_eq!(
        adapter.report_child("a", ChildChange::Update, firm(State::Working)),
        Err(AdapterRefusal::UnknownChild)
    );
    assert_eq!(
        start(&mut adapter, "b", Some("missing")),
        Err(AdapterRefusal::UnknownParent)
    );
    start(&mut adapter, "a", None).unwrap();
    assert_eq!(
        adapter.report_child("a", ChildChange::End, firm(State::Working)),
        Err(AdapterRefusal::WrongState),
        "an end needs a result"
    );
    assert_eq!(
        adapter.report_child("a", ChildChange::Update, firm(State::Idle)),
        Err(AdapterRefusal::WrongState)
    );
    adapter
        .report_child("a", ChildChange::End, firm(State::Error))
        .unwrap();
    let ended = &adapter.report().children[0].observation;
    assert_eq!(ended.state, State::Error);
}
