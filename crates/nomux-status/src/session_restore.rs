//! The rule for a session that lost its process.
//!
//! When the holder of a session is gone, for example after a restart of the
//! host, its sources are gone too. Work that ran does not run any more, and
//! work that waited for the user has no program that takes the answer. To
//! show such work as running or as waiting is false. To show nothing loses
//! the fact that work was there. Thus the work becomes complete work with
//! the source [`Source::SessionRestore`], and a result that the user did
//! not see stays.

use crate::agent_run::{AgentRun, ChildWork};
use crate::observation::{ChildObservation, Observation, SourceReport};
use crate::source::Source;
use crate::state::State;
use crate::status::Status;

/// What stays of `run` after its session lost its process, as the report of
/// the source [`Source::SessionRestore`].
///
/// - Work that ran or that waited for the user is complete work. Its
///   message and its progress described it while it ran, thus they do not
///   stay.
/// - A result (complete work or a failure) stays as it is.
/// - Idle work and a guess do not stay: there is nothing to keep.
///
/// Returns `None` if nothing stays.
#[must_use]
pub fn carried_over(run: &AgentRun) -> Option<SourceReport> {
    let mut children = Vec::new();
    for child in &run.children {
        carry_children(child, &mut Vec::new(), &mut children);
    }
    let root = carried(&run.status);
    if root.is_none() && children.is_empty() {
        return None;
    }
    Some(SourceReport {
        source: Source::SessionRestore,
        root,
        children,
        note: "the session lost its process: the status is what the session had then, and work \
               that ran or waited is shown as complete"
            .to_owned(),
    })
}

fn carry_children(
    child: &ChildWork,
    parent: &mut Vec<String>,
    carried_over: &mut Vec<ChildObservation>,
) {
    parent.push(child.name.clone());
    if let Some(observation) = child.status.as_ref().and_then(carried) {
        carried_over.push(ChildObservation {
            path: parent.clone(),
            observation,
        });
    }
    for below in &child.children {
        carry_children(below, parent, carried_over);
    }
    parent.pop();
}

fn carried(status: &Status) -> Option<Observation> {
    let reported = status.source.is_some_and(|source| !source.is_guess());
    if !reported || status.state == State::Idle {
        return None;
    }
    let was_live = status.state.needs_process();
    let value =
        |text: &Option<crate::Sourced<String>>| text.as_ref().map(|text| text.value.clone());
    Some(Observation {
        state: if was_live { State::Done } else { status.state },
        message: if was_live {
            None
        } else {
            value(&status.message)
        },
        title: value(&status.title),
        progress: None,
        tool: value(&status.tool),
    })
}
