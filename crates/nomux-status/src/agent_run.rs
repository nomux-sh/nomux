//! The status of what runs in one session: an Agent run and the tree of its
//! Child work.

use crate::aggregation::most_urgent;
use crate::status::{Status, Timestamp};

/// The largest number of levels of Child work below an Agent run.
pub const MAX_DEPTH: usize = 8;

/// The largest number of Child work items with a status in one Agent run.
/// The resolver ignores the observations after them, thus a source cannot
/// make an Agent run grow without a limit.
pub const MAX_CHILD_WORK: usize = 64;

/// One Child work: a subagent, a teammate or a background task below an
/// Agent run or below a Child work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildWork {
    /// The name of the Child work below its parent.
    pub name: String,
    /// The status of this Child work. `None` if no source reports it: it is
    /// in the tree only because a Child work below it has a status.
    pub status: Option<Status>,
    /// The most urgent status of this Child work and of each Child work
    /// below it.
    pub aggregate: Status,
    /// The Child work below this one, in the sequence of their names.
    pub children: Vec<ChildWork>,
}

/// The status of the Agent run of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    /// The status of the Agent run.
    pub status: Status,
    /// The most urgent status of the Agent run and of each Child work below
    /// it.
    pub aggregate: Status,
    /// The Child work of the Agent run, in the sequence of their names.
    pub children: Vec<ChildWork>,
}

impl AgentRun {
    /// An Agent run with `status` and `children`. The aggregates of
    /// `children` must be correct.
    pub(crate) fn new(status: Status, children: Vec<ChildWork>) -> Self {
        let aggregate = aggregate_of(Some(&status), &children);
        Self {
            status,
            aggregate,
            children,
        }
    }

    /// The Agent run of a session that no source reports.
    #[must_use]
    pub fn unreported(now: Timestamp) -> Self {
        Self::new(Status::unreported(now), Vec::new())
    }

    /// The status of the Child work at `path`. Returns `None` if the tree
    /// has no Child work there, or if no source reports it.
    #[must_use]
    pub fn child_status(&self, path: &[String]) -> Option<&Status> {
        let (last, parents) = path.split_last()?;
        let mut level = &self.children;
        for name in parents {
            level = &find(level, name)?.children;
        }
        find(level, last)?.status.as_ref()
    }

    /// Records that the user saw each result of the Agent run and of its
    /// Child work. Returns `true` if a status changed.
    pub fn mark_seen(&mut self) -> bool {
        self.change_each_status(&mut Status::mark_seen)
    }

    /// Changes each status of the tree with `change`, and then makes the
    /// aggregates correct again. Returns `true` if `change` changed a
    /// status.
    fn change_each_status(&mut self, change: &mut impl FnMut(&mut Status) -> bool) -> bool {
        let mut changed = change(&mut self.status);
        for child in &mut self.children {
            changed |= child.change_each_status(change);
        }
        self.aggregate = aggregate_of(Some(&self.status), &self.children);
        changed
    }
}

impl ChildWork {
    /// A Child work with `status` and `children`. It has a status or one
    /// child at least. The aggregates of `children` must be correct.
    pub(crate) fn new(name: String, status: Option<Status>, children: Vec<Self>) -> Self {
        let aggregate = aggregate_of(status.as_ref(), &children);
        Self {
            name,
            status,
            aggregate,
            children,
        }
    }

    fn change_each_status(&mut self, change: &mut impl FnMut(&mut Status) -> bool) -> bool {
        let mut changed = self.status.as_mut().is_some_and(&mut *change);
        for child in &mut self.children {
            changed |= child.change_each_status(change);
        }
        self.aggregate = aggregate_of(self.status.as_ref(), &self.children);
        changed
    }
}

fn find<'tree>(level: &'tree [ChildWork], name: &str) -> Option<&'tree ChildWork> {
    level.iter().find(|child| child.name == name)
}

/// The most urgent status of an item and of its children. The status of the
/// item is first, thus it wins against a child with the same urgency and
/// the same `since`.
fn aggregate_of(status: Option<&Status>, children: &[ChildWork]) -> Status {
    let candidates = status
        .into_iter()
        .chain(children.iter().map(|child| &child.aggregate))
        .map(|status| ((), status));
    most_urgent(candidates)
        .map(|((), status)| status.clone())
        .expect("an item has a status or a child")
}
