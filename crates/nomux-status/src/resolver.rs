//! The resolver: merges what the sources report into one Agent run.

use std::collections::BTreeMap;

use crate::agent_run::{AgentRun, ChildWork, MAX_CHILD_WORK, MAX_DEPTH};
use crate::observation::{Observation, SourceReport};
use crate::source::{Source, Sourced};
use crate::status::{Status, Timestamp};

/// What the sources say about one item, in the sequence in which the
/// sources decide.
type Candidates<'report> = Vec<(Source, &'report Observation)>;

/// Merges `reports` into the Agent run of a session.
///
/// - The sources decide in the sequence of [`Source`]. The sequence of
///   `reports` has no effect.
/// - The first source that reports an item gives its state. The message and
///   the progress come from the first source that has them and that reports
///   the same state: a text of a different state describes a different
///   state. The title and the name of the tool come from the first source
///   that has them.
/// - An item keeps its `since` and its `seen` from `previous` while its
///   state does not change. An item with a new state has `now` and was not
///   seen.
/// - A Child work below [`MAX_DEPTH`] levels is ignored, and so is each
///   Child work after the first [`MAX_CHILD_WORK`].
#[must_use]
pub fn resolve(reports: &[SourceReport], previous: Option<&AgentRun>, now: Timestamp) -> AgentRun {
    let mut reports: Vec<&SourceReport> = reports.iter().collect();
    reports.sort_by_key(|report| report.source);

    let root: Candidates<'_> = reports
        .iter()
        .filter_map(|report| Some((report.source, report.root.as_ref()?)))
        .collect();
    let status = merge(&root, previous.map(|run| &run.status), now);

    let statuses: BTreeMap<&[String], Status> = child_candidates(&reports)
        .into_iter()
        .map(|(path, candidates)| {
            let before = previous.and_then(|run| run.child_status(path));
            (path, merge(&candidates, before, now))
        })
        .collect();
    AgentRun::new(status, children_below(&[], &statuses))
}

/// The candidates of each Child work that a source reports, by its path.
fn child_candidates<'report>(
    reports: &[&'report SourceReport],
) -> BTreeMap<&'report [String], Candidates<'report>> {
    let mut by_path: BTreeMap<&[String], Candidates<'_>> = BTreeMap::new();
    for report in reports {
        for child in &report.children {
            let path = child.path.as_slice();
            let in_tree = !path.is_empty() && path.len() <= MAX_DEPTH;
            let has_room = by_path.len() < MAX_CHILD_WORK || by_path.contains_key(path);
            if in_tree && has_room {
                by_path
                    .entry(path)
                    .or_default()
                    .push((report.source, &child.observation));
            }
        }
    }
    by_path
}

/// The status of one item from its candidates.
fn merge(candidates: &Candidates<'_>, previous: Option<&Status>, now: Timestamp) -> Status {
    let Some((source, decisive)) = candidates.first() else {
        return match previous {
            Some(previous) if previous.source.is_none() => previous.clone(),
            _ => Status::unreported(now),
        };
    };
    let state = decisive.state;
    let same_state = |candidate: &&(Source, &Observation)| candidate.1.state.is_same_state(state);
    let unchanged = previous.filter(|previous| previous.state == state);
    Status {
        state,
        source: Some(*source),
        since: unchanged.map_or(now, |previous| previous.since),
        seen: unchanged.is_some_and(|previous| previous.seen),
        message: first(candidates.iter().filter(same_state), |observation| {
            observation.message.clone()
        }),
        progress: first(candidates.iter().filter(same_state), |observation| {
            observation.progress
        }),
        title: first(candidates, |observation| observation.title.clone()),
        tool: first(candidates, |observation| observation.tool.clone()),
    }
}

/// The value of the first candidate that has one, with its source.
fn first<'candidate, 'report: 'candidate, T>(
    candidates: impl IntoIterator<Item = &'candidate (Source, &'report Observation)>,
    value: impl Fn(&Observation) -> Option<T>,
) -> Option<Sourced<T>> {
    candidates.into_iter().find_map(|(source, observation)| {
        Some(Sourced {
            value: value(observation)?,
            source: *source,
        })
    })
}

/// The Child work directly below `parent`, in the sequence of their names.
/// A path without a status of its own is in the tree if a path below it has
/// a status.
fn children_below(parent: &[String], statuses: &BTreeMap<&[String], Status>) -> Vec<ChildWork> {
    let mut names: Vec<&String> = statuses
        .keys()
        .filter(|path| path.len() > parent.len() && path.starts_with(parent))
        .map(|path| &path[parent.len()])
        .collect();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let path = [parent, std::slice::from_ref(name)].concat();
            let status = statuses.get(path.as_slice()).cloned();
            ChildWork::new(name.clone(), status, children_below(&path, statuses))
        })
        .collect()
}
