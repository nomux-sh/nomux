//! The state, the flags and the refusals of a worktree, from its git facts
//! and from what the daemon sees on the host. `spec/worktree.md` of the
//! `proto` repository has the rules. This module only applies them: it reads
//! nothing.

use std::time::Duration;

use nomux_proto::wire::v1::{
    WorktreeFacts, WorktreeFlag, WorktreeMark, WorktreeMergedBy, WorktreeRefusal, WorktreeState,
};

/// What the daemon sees of a worktree on the host.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct HostView {
    /// A session that is not ended is in the worktree.
    pub(super) has_session: bool,
    /// The flags that the daemon finds itself: `Busy`, `OtherFileSystem`
    /// and `IgnoredUserFiles`.
    pub(super) flags: Vec<WorktreeFlag>,
    /// The mark of the user.
    pub(super) mark: WorktreeMark,
    /// The worktree is in the trash place.
    pub(super) in_trash: bool,
    /// The parent directory of the worktree is not writable.
    pub(super) no_trash_place: bool,
}

/// The state, the flags and the refusals of one worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Judgement {
    pub(super) state: WorktreeState,
    pub(super) flags: Vec<WorktreeFlag>,
    pub(super) refusals: Vec<WorktreeRefusal>,
}

/// Applies the rules to one worktree at the time `now_ms`. A worktree
/// without activity for `stale_after` is stale.
pub(super) fn judge(
    facts: &WorktreeFacts,
    view: &HostView,
    now_ms: u64,
    stale_after: Duration,
) -> Judgement {
    let state = state(facts, view, now_ms, stale_after);
    let flags = flags(facts, view);
    let refusals = refusals(facts, view, state, &flags);
    Judgement {
        state,
        flags,
        refusals,
    }
}

fn state(
    facts: &WorktreeFacts,
    view: &HostView,
    now_ms: u64,
    stale_after: Duration,
) -> WorktreeState {
    let stale_after = u64::try_from(stale_after.as_millis()).unwrap_or(u64::MAX);
    if view.has_session {
        WorktreeState::Active
    } else if facts.merged_by() != WorktreeMergedBy::Unspecified
        || view.mark == WorktreeMark::Finished
    {
        WorktreeState::Finished
    } else if now_ms.saturating_sub(facts.last_activity_at_ms) > stale_after {
        WorktreeState::Stale
    } else {
        WorktreeState::Idle
    }
}

fn flags(facts: &WorktreeFacts, view: &HostView) -> Vec<WorktreeFlag> {
    let candidates = [
        (facts.is_dirty, WorktreeFlag::Dirty),
        (facts.unpushed_commits > 0, WorktreeFlag::Unpushed),
        (facts.has_detached_commits, WorktreeFlag::DetachedCommits),
        (facts.is_locked, WorktreeFlag::Locked),
        (facts.has_submodules, WorktreeFlag::Submodule),
        (facts.is_missing, WorktreeFlag::Missing),
        (facts.upstream_gone, WorktreeFlag::UpstreamGone),
    ];
    let mut flags: Vec<WorktreeFlag> = candidates
        .into_iter()
        .filter_map(|(set, flag)| set.then_some(flag))
        .chain(view.flags.iter().copied())
        .collect();
    flags.sort();
    flags.dedup();
    flags
}

fn refusals(
    facts: &WorktreeFacts,
    view: &HostView,
    state: WorktreeState,
    flags: &[WorktreeFlag],
) -> Vec<WorktreeRefusal> {
    let has = |flag| flags.contains(&flag);
    // With these rules the base branch has each change of the branch: a
    // commit that is not pushed loses nothing.
    let changes_in_base = matches!(
        facts.merged_by(),
        WorktreeMergedBy::Ancestor | WorktreeMergedBy::Tree
    );
    let candidates = [
        (facts.is_main, WorktreeRefusal::MainWorktree),
        (
            has(WorktreeFlag::Missing),
            WorktreeRefusal::DirectoryMissing,
        ),
        (!facts.error.is_empty(), WorktreeRefusal::FactsUnknown),
        (
            state == WorktreeState::Active,
            WorktreeRefusal::ActiveSession,
        ),
        (has(WorktreeFlag::Busy), WorktreeRefusal::ProcessRunning),
        (
            has(WorktreeFlag::Dirty),
            WorktreeRefusal::UncommittedChanges,
        ),
        (
            has(WorktreeFlag::Unpushed) && !changes_in_base,
            WorktreeRefusal::UnpushedCommits,
        ),
        (
            has(WorktreeFlag::DetachedCommits),
            WorktreeRefusal::DetachedCommits,
        ),
        // The lock of the trash place is the lock of nomux.
        (
            has(WorktreeFlag::Locked) && !view.in_trash,
            WorktreeRefusal::Locked,
        ),
        (has(WorktreeFlag::Submodule), WorktreeRefusal::Submodule),
        (
            has(WorktreeFlag::IgnoredUserFiles),
            WorktreeRefusal::IgnoredUserFiles,
        ),
        (
            has(WorktreeFlag::OtherFileSystem),
            WorktreeRefusal::OtherFileSystem,
        ),
        (view.no_trash_place, WorktreeRefusal::NoTrashPlace),
        (view.in_trash, WorktreeRefusal::InTrash),
        (view.mark == WorktreeMark::Keep, WorktreeRefusal::Kept),
        (
            state != WorktreeState::Finished,
            WorktreeRefusal::NotFinished,
        ),
    ];
    candidates
        .into_iter()
        .filter_map(|(set, refusal)| set.then_some(refusal))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 100 * 24 * 60 * 60 * 1000;
    const STALE_AFTER: Duration = Duration::from_hours(14 * 24);
    const DAY: u64 = 24 * 60 * 60 * 1000;

    /// A linked worktree, merged by a merge, active yesterday.
    fn finished() -> WorktreeFacts {
        WorktreeFacts {
            path: "/w".to_owned(),
            branch: "b".to_owned(),
            last_activity_at_ms: NOW - DAY,
            merged_by: WorktreeMergedBy::Ancestor.into(),
            ..WorktreeFacts::default()
        }
    }

    fn not_merged() -> WorktreeFacts {
        WorktreeFacts {
            merged_by: WorktreeMergedBy::Unspecified.into(),
            ..finished()
        }
    }

    fn with_flags(flags: &[WorktreeFlag]) -> HostView {
        HostView {
            flags: flags.to_vec(),
            ..HostView::default()
        }
    }

    /// A finished worktree with one change.
    fn with(change: impl FnOnce(&mut WorktreeFacts)) -> WorktreeFacts {
        let mut facts = finished();
        change(&mut facts);
        facts
    }

    fn session() -> HostView {
        HostView {
            has_session: true,
            ..HostView::default()
        }
    }

    type Case = (
        &'static str,
        WorktreeFacts,
        HostView,
        WorktreeState,
        Option<WorktreeRefusal>,
    );

    fn assert_cases(cases: Vec<Case>) {
        for (name, facts, view, state, first_refusal) in cases {
            let judgement = judge(&facts, &view, NOW, STALE_AFTER);
            assert_eq!(judgement.state, state, "{name}");
            assert_eq!(judgement.refusals.first().copied(), first_refusal, "{name}");
        }
    }

    #[test]
    fn the_git_facts_give_the_state_and_the_first_refusal() {
        use WorktreeRefusal as R;
        use WorktreeState as S;
        let none = HostView::default;
        let open = |change: fn(&mut WorktreeFacts)| {
            let mut facts = not_merged();
            change(&mut facts);
            facts
        };
        assert_cases(vec![
            ("merged and clean", finished(), none(), S::Finished, None),
            (
                "main",
                with(|f| f.is_main = true),
                none(),
                S::Finished,
                Some(R::MainWorktree),
            ),
            (
                "missing",
                open(|f| f.is_missing = true),
                none(),
                S::Idle,
                Some(R::DirectoryMissing),
            ),
            (
                "unknown",
                open(|f| f.error = "e".to_owned()),
                none(),
                S::Idle,
                Some(R::FactsUnknown),
            ),
            (
                "dirty",
                with(|f| f.is_dirty = true),
                none(),
                S::Finished,
                Some(R::UncommittedChanges),
            ),
            (
                "detached",
                open(|f| f.has_detached_commits = true),
                none(),
                S::Idle,
                Some(R::DetachedCommits),
            ),
            (
                "locked",
                with(|f| f.is_locked = true),
                none(),
                S::Finished,
                Some(R::Locked),
            ),
            (
                "submodule",
                with(|f| f.has_submodules = true),
                none(),
                S::Finished,
                Some(R::Submodule),
            ),
            (
                "open, recent",
                not_merged(),
                none(),
                S::Idle,
                Some(R::NotFinished),
            ),
            (
                "15 days",
                open(|f| f.last_activity_at_ms = NOW - 15 * DAY),
                none(),
                S::Stale,
                Some(R::NotFinished),
            ),
            (
                "13 days",
                open(|f| f.last_activity_at_ms = NOW - 13 * DAY),
                none(),
                S::Idle,
                Some(R::NotFinished),
            ),
        ]);
    }

    #[test]
    fn commits_that_are_not_pushed_are_refused_unless_the_base_branch_has_their_changes() {
        use WorktreeRefusal as R;
        use WorktreeState as S;
        let gone = with(|f| {
            f.unpushed_commits = 2;
            f.merged_by = WorktreeMergedBy::UpstreamGone.into();
        });
        let in_base = with(|f| {
            f.unpushed_commits = 2;
            f.merged_by = WorktreeMergedBy::Tree.into();
        });
        assert_cases(vec![
            (
                "gone",
                gone,
                HostView::default(),
                S::Finished,
                Some(R::UnpushedCommits),
            ),
            (
                "in the base branch",
                in_base,
                HostView::default(),
                S::Finished,
                None,
            ),
        ]);
    }

    #[test]
    fn what_the_daemon_sees_on_the_host_gives_the_state_and_the_first_refusal() {
        use WorktreeFlag as F;
        use WorktreeRefusal as R;
        use WorktreeState as S;
        assert_cases(vec![
            (
                "a session",
                finished(),
                session(),
                S::Active,
                Some(R::ActiveSession),
            ),
            (
                "busy",
                finished(),
                with_flags(&[F::Busy]),
                S::Finished,
                Some(R::ProcessRunning),
            ),
            (
                "user files",
                finished(),
                with_flags(&[F::IgnoredUserFiles]),
                S::Finished,
                Some(R::IgnoredUserFiles),
            ),
            (
                "other file system",
                finished(),
                with_flags(&[F::OtherFileSystem]),
                S::Finished,
                Some(R::OtherFileSystem),
            ),
            (
                "a session in an open worktree",
                not_merged(),
                session(),
                S::Active,
                Some(R::ActiveSession),
            ),
        ]);
    }

    #[test]
    fn a_mark_and_the_trash_place_change_the_state_and_the_refusals() {
        use WorktreeRefusal as R;
        use WorktreeState as S;
        let view = |change: fn(&mut HostView)| {
            let mut view = HostView::default();
            change(&mut view);
            view
        };
        let in_trash = with(|f| {
            f.is_locked = true;
            f.locked_reason = "nomux-trash".to_owned();
        });
        assert_cases(vec![
            (
                "marked done",
                not_merged(),
                view(|v| v.mark = WorktreeMark::Finished),
                S::Finished,
                None,
            ),
            (
                "marked keep",
                finished(),
                view(|v| v.mark = WorktreeMark::Keep),
                S::Finished,
                Some(R::Kept),
            ),
            (
                "in the trash: its lock is the lock of nomux",
                in_trash,
                view(|v| v.in_trash = true),
                S::Finished,
                Some(R::InTrash),
            ),
            (
                "a parent that is not writable",
                finished(),
                view(|v| v.no_trash_place = true),
                S::Finished,
                Some(R::NoTrashPlace),
            ),
        ]);
    }

    #[test]
    fn the_flags_come_from_the_facts_and_the_host_once_in_the_sequence_of_their_values() {
        let facts = WorktreeFacts {
            is_dirty: true,
            unpushed_commits: 1,
            is_locked: true,
            upstream_gone: true,
            ..finished()
        };
        let view = with_flags(&[WorktreeFlag::OtherFileSystem, WorktreeFlag::Busy]);

        let judgement = judge(&facts, &view, NOW, STALE_AFTER);

        assert_eq!(
            judgement.flags,
            [
                WorktreeFlag::Dirty,
                WorktreeFlag::Unpushed,
                WorktreeFlag::Locked,
                WorktreeFlag::Busy,
                WorktreeFlag::OtherFileSystem,
                WorktreeFlag::UpstreamGone,
            ]
        );
        // Each reason of a refusal, the most important first.
        assert_eq!(
            judgement.refusals,
            [
                WorktreeRefusal::ProcessRunning,
                WorktreeRefusal::UncommittedChanges,
                WorktreeRefusal::Locked,
                WorktreeRefusal::OtherFileSystem,
            ]
        );
    }
}
