//! The adapter source: what an adapter extension reported about the Agent
//! run of one session and about its Child work.
//!
//! An adapter reports from the events of an agent tool. A tool can send the
//! event of one tool call after the end of the work or while it waits for
//! the user, because its hooks run in parallel. Thus a soft working report
//! does not replace a state that waits for the user, and it does not replace
//! complete work whose Child work still runs (work in the background).

use std::error::Error;
use std::fmt;

use crate::agent_run::{MAX_CHILD_WORK, MAX_DEPTH};
use crate::observation::{ChildObservation, Observation, SourceReport};
use crate::source::Source;
use crate::state::State;

/// What happens to one Child work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildChange {
    /// The Child work starts below `parent`, or below the Agent run if
    /// `parent` is `None`. A start of a known Child work changes its status
    /// and makes it run again.
    Start {
        /// The identifier of the parent Child work.
        parent: Option<String>,
    },
    /// The status of a known Child work changes.
    Update,
    /// A known Child work ends with a result.
    End,
}

/// Why the adapter source did not take a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterRefusal {
    /// The report names a Child work that did not start.
    UnknownChild,
    /// The start names a parent that did not start.
    UnknownParent,
    /// The Child work would be below [`MAX_DEPTH`] levels.
    TooDeep,
    /// [`MAX_CHILD_WORK`] Child work items run, and none ended.
    TooMany,
    /// The state does not fit the change: a start or an update must not be
    /// idle, an end must be a result.
    WrongState,
}

impl fmt::Display for AdapterRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownChild => "the Child work did not start",
            Self::UnknownParent => "the parent Child work did not start",
            Self::TooDeep => "the Child work would be below 8 levels",
            Self::TooMany => "64 Child work items run, and none ended",
            Self::WrongState => {
                "a start or an update needs a state that is not idle, and an end needs done or \
                 error"
            }
        })
    }
}

impl Error for AdapterRefusal {}

/// What one adapter reported about one session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdapterSource {
    root: Option<Observation>,
    /// In the sequence of their start: the first one is the oldest.
    children: Vec<AdapterChild>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdapterChild {
    id: String,
    parent: Option<String>,
    observation: Observation,
    ended: bool,
}

impl AdapterSource {
    /// The adapter reports the Agent run. [`State::Idle`] takes the report
    /// of the Agent run back.
    pub fn report_run(&mut self, observation: Observation) {
        if observation.state == State::Idle {
            self.root = None;
            return;
        }
        let works_below = self.works_below(None);
        if keeps(self.root.as_ref(), &observation, works_below) {
            return;
        }
        self.root = Some(observation);
    }

    /// The adapter reports a change of the Child work `id`.
    ///
    /// # Errors
    ///
    /// Returns the reason if the change does not fit the tree of the Child
    /// work or the state does not fit the change.
    pub fn report_child(
        &mut self,
        id: &str,
        change: ChildChange,
        observation: Observation,
    ) -> Result<(), AdapterRefusal> {
        let state_fits = match change {
            ChildChange::End => observation.state.is_result(),
            ChildChange::Start { .. } | ChildChange::Update => observation.state != State::Idle,
        };
        if !state_fits {
            return Err(AdapterRefusal::WrongState);
        }
        match change {
            ChildChange::Start { parent } => self.start(id, parent, observation),
            ChildChange::Update => self.update(id, observation, false),
            ChildChange::End => self.update(id, observation, true),
        }
    }

    /// What the adapter says, as the report of [`Source::Adapter`].
    #[must_use]
    pub fn report(&self) -> SourceReport {
        let children: Vec<ChildObservation> = self
            .children
            .iter()
            .map(|child| ChildObservation {
                path: self.path_of(child),
                observation: child.observation.clone(),
            })
            .collect();
        let note = if self.root.is_none() && children.is_empty() {
            "no adapter extension reported a status"
        } else {
            "an adapter extension reported it from the events of the agent tool"
        };
        SourceReport {
            source: Source::Adapter,
            root: self.root.clone(),
            children,
            note: note.to_owned(),
        }
    }

    fn start(
        &mut self,
        id: &str,
        parent: Option<String>,
        observation: Observation,
    ) -> Result<(), AdapterRefusal> {
        if self.find(id).is_some() {
            return self.update(id, observation, false);
        }
        let depth = match &parent {
            None => 1,
            Some(parent) => 1 + self.depth_of(parent).ok_or(AdapterRefusal::UnknownParent)?,
        };
        if depth > MAX_DEPTH {
            return Err(AdapterRefusal::TooDeep);
        }
        if self.children.len() >= MAX_CHILD_WORK {
            self.forget_oldest_ended()?;
        }
        self.children.push(AdapterChild {
            id: id.to_owned(),
            parent,
            observation,
            ended: false,
        });
        Ok(())
    }

    fn update(
        &mut self,
        id: &str,
        observation: Observation,
        ends: bool,
    ) -> Result<(), AdapterRefusal> {
        let index = self.find(id).ok_or(AdapterRefusal::UnknownChild)?;
        let works_below = self.works_below(Some(id));
        let child = &mut self.children[index];
        if !keeps(Some(&child.observation), &observation, works_below) {
            child.observation = observation;
            child.ended = ends;
        }
        Ok(())
    }

    /// Removes the oldest Child work that ended and has no Child work below
    /// it, to make room for a new one.
    fn forget_oldest_ended(&mut self) -> Result<(), AdapterRefusal> {
        let is_parent = |id: &str| {
            self.children
                .iter()
                .any(|child| child.parent.as_deref() == Some(id))
        };
        let oldest = self
            .children
            .iter()
            .position(|child| child.ended && !is_parent(&child.id))
            .ok_or(AdapterRefusal::TooMany)?;
        self.children.remove(oldest);
        Ok(())
    }

    fn find(&self, id: &str) -> Option<usize> {
        self.children.iter().position(|child| child.id == id)
    }

    /// The number of levels from the Agent run down to the Child work `id`.
    fn depth_of(&self, id: &str) -> Option<usize> {
        let child = &self.children[self.find(id)?];
        Some(self.path_of(child).len())
    }

    /// The identifiers from the Agent run down to `child`. A start checks
    /// that each parent is known, thus the path ends at the Agent run.
    fn path_of(&self, child: &AdapterChild) -> Vec<String> {
        let mut path = vec![child.id.clone()];
        let mut parent = child.parent.as_deref();
        while let Some(id) = parent {
            path.push(id.to_owned());
            parent = self
                .find(id)
                .and_then(|index| self.children[index].parent.as_deref());
        }
        path.reverse();
        path
    }

    /// `true` if a Child work below `parent` (below the Agent run for
    /// `None`) works.
    fn works_below(&self, parent: Option<&str>) -> bool {
        self.children
            .iter()
            .filter(|child| child.parent.as_deref() == parent)
            .any(|child| {
                child.observation.state == State::Working || self.works_below(Some(&child.id))
            })
    }
}

/// `true` if `current` stays against the report `new`: a soft working
/// report does not replace a state that needs the user, and it does not
/// replace complete work while Child work below it works.
fn keeps(current: Option<&Observation>, new: &Observation, works_below: bool) -> bool {
    let Some(current) = current else {
        return false;
    };
    if !new.is_soft_working() {
        return false;
    }
    match current.state {
        State::NeedsYou(_) => true,
        State::Done => works_below,
        State::Idle | State::Working | State::Error => false,
    }
}
