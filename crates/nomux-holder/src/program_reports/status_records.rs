//! The program status records as a tree: the `id` of a record is a path,
//! and `build/test` is below `build`, which is below the root record (the
//! empty `id`). A record above a record does not have to exist.

use nomux_holder_protocol::program_reports::ProgramStatus;
use nomux_term::osc::ProgramStatusReport;

/// The `app` value that a report claims: its own value, or the value of the
/// nearest record above its `id` that has one. This is the value that the
/// record of the report gets from the specification of OSC 7501.
pub(super) fn claimed_app<'a>(
    statuses: &'a [ProgramStatus],
    report: &'a ProgramStatusReport,
) -> Option<&'a str> {
    report.app.as_deref().or_else(|| {
        std::iter::successors(parent(&report.id), |id| parent(id)).find_map(|id| {
            let record = statuses.iter().find(|record| record.id == id)?;
            record.app.as_deref()
        })
    })
}

/// Removes the record `id` and each record below it. An empty `id` removes
/// all records. Returns `true` if a record was removed.
pub(super) fn clear(statuses: &mut Vec<ProgramStatus>, id: &str) -> bool {
    let count = statuses.len();
    statuses.retain(|record| !is_at_or_below(&record.id, id));
    statuses.len() != count
}

/// Puts `record` in the place of the record with the same `id`, or adds it.
/// If there are then more than `max_records`, removes the record with the
/// oldest report.
pub(super) fn replace(
    statuses: &mut Vec<ProgramStatus>,
    record: ProgramStatus,
    max_records: usize,
) {
    match statuses
        .iter_mut()
        .find(|existing| existing.id == record.id)
    {
        Some(existing) => *existing = record,
        None => statuses.push(record),
    }
    if statuses.len() > max_records {
        let oldest = statuses
            .iter()
            .enumerate()
            .min_by_key(|(_, record)| record.revision)
            .map(|(index, _)| index);
        if let Some(oldest) = oldest {
            statuses.remove(oldest);
        }
    }
}

/// The `id` of the record directly above `id`. `None` for the root record.
fn parent(id: &str) -> Option<&str> {
    if id.is_empty() {
        return None;
    }
    Some(id.rsplit_once('/').map_or("", |(parent, _)| parent))
}

fn is_at_or_below(id: &str, ancestor: &str) -> bool {
    ancestor.is_empty()
        || id
            .strip_prefix(ancestor)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}
