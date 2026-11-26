//! What an adapter extension reports with the host services `agent.report`
//! and `agent.child`, after its checks. `spec/adapter.md` of the `proto`
//! repository has the rules.

use std::error::Error;
use std::fmt;

use nomux_proto::wire::v1 as wire;
use nomux_status::{AdapterRefusal, ChildChange, Observation, State};

use super::message::reported_state_and_kind;
use super::report_text::one_line;

/// The largest number of bytes of the identifier of a Child work.
const MAX_CHILD_ID_LENGTH: usize = 128;

/// A report of the Agent run of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunReport {
    pub(crate) observation: Observation,
    /// `None` if the details do not change.
    pub(crate) details: Option<wire::AgentDetails>,
}

/// A report of one Child work of the Agent run of a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildReport {
    pub(crate) id: String,
    pub(crate) change: ChildChange,
    pub(crate) observation: Observation,
}

/// Why the daemon did not take a report of an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterReportRefused {
    /// The session has no process: its status comes from the sources of
    /// its next process.
    NoProcess,
    /// The report does not fit the Child work of the session.
    Refused(AdapterRefusal),
}

impl fmt::Display for AdapterReportRefused {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProcess => formatter.write_str("the session has no process"),
            Self::Refused(refusal) => refusal.fmt(formatter),
        }
    }
}

impl Error for AdapterReportRefused {}

impl RunReport {
    /// The report of `report`. The caller has the session identifier.
    ///
    /// # Errors
    ///
    /// Returns the reason if a part of the report is not valid.
    pub(crate) fn new(report: &wire::AgentReport) -> Result<Self, String> {
        let observation = observation(
            report.state,
            report.kind,
            report.soft,
            [&report.message, &report.title, &report.tool],
        )?;
        let details = report.details.as_ref().map(checked_details).transpose()?;
        Ok(Self {
            observation,
            details,
        })
    }
}

impl ChildReport {
    /// The report of `report`. The caller has the session identifier.
    ///
    /// # Errors
    ///
    /// Returns the reason if a part of the report is not valid.
    pub(crate) fn new(report: &wire::ChildWorkReport) -> Result<Self, String> {
        let id = child_id("identifier", &report.id)?;
        let change = match wire::ChildWorkChange::try_from(report.change) {
            Ok(wire::ChildWorkChange::Start) => ChildChange::Start {
                parent: (!report.parent_id.is_empty())
                    .then(|| child_id("parent identifier", &report.parent_id))
                    .transpose()?,
            },
            Ok(wire::ChildWorkChange::Update) => ChildChange::Update,
            Ok(wire::ChildWorkChange::End) => ChildChange::End,
            Ok(wire::ChildWorkChange::Unspecified) | Err(_) => {
                return Err(format!("{} is not a change of a Child work", report.change));
            }
        };
        let observation = observation(
            report.state,
            report.kind,
            report.soft,
            [&report.message, &report.title, &report.tool],
        )?;
        Ok(Self {
            id,
            change,
            observation,
        })
    }
}

/// The observation of a state, a kind and the texts `[message, title,
/// tool]`.
fn observation(
    state: i32,
    kind: i32,
    soft: bool,
    [message, title, tool]: [&String; 3],
) -> Result<Observation, String> {
    let state: State = reported_state_and_kind(state, kind)
        .ok_or_else(|| format!("{state} is not a state of an Agent run"))?;
    Ok(Observation {
        message: one_line("message", message)?,
        title: one_line("title", title)?,
        tool: one_line("tool", tool)?,
        soft,
        ..Observation::of(state)
    })
}

fn checked_details(details: &wire::AgentDetails) -> Result<wire::AgentDetails, String> {
    for (name, text) in [
        ("model", &details.model),
        ("provider session identifier", &details.provider_session_id),
        ("resume command", &details.resume_command),
    ] {
        one_line(name, text)?;
    }
    Ok(details.clone())
}

/// The identifier of a Child work: it is a name in the path of the tree.
fn child_id(name: &str, id: &str) -> Result<String, String> {
    let valid = !id.is_empty()
        && id.len() <= MAX_CHILD_ID_LENGTH
        && !id.contains('/')
        && !id.chars().any(char::is_control);
    if !valid {
        return Err(format!(
            "the {name} of a Child work must have 1 to {MAX_CHILD_ID_LENGTH} bytes without `/` \
             and without a control character"
        ));
    }
    Ok(id.to_owned())
}

#[cfg(test)]
mod tests {
    use nomux_status::NeedsYouKind;

    use super::*;

    #[test]
    fn a_report_needs_a_known_state_and_short_lines() {
        let report = |state: wire::AgentState, message: &str| {
            RunReport::new(&wire::AgentReport {
                state: state.into(),
                kind: wire::NeedsYouKind::Question.into(),
                message: message.to_owned(),
                ..wire::AgentReport::default()
            })
        };

        let needs_you = report(wire::AgentState::NeedsYou, "which file?").unwrap();
        assert_eq!(
            needs_you.observation.state,
            State::NeedsYou(Some(NeedsYouKind::Question))
        );
        assert!(report(wire::AgentState::Unspecified, "").is_err());
        assert!(report(wire::AgentState::Working, "two\nlines").is_err());
        let long_details = wire::AgentReport {
            state: wire::AgentState::Working.into(),
            details: Some(wire::AgentDetails {
                resume_command: "r".repeat(513),
                ..wire::AgentDetails::default()
            }),
            ..wire::AgentReport::default()
        };
        assert!(RunReport::new(&long_details).is_err());
    }

    #[test]
    fn a_child_work_identifier_is_one_name_of_a_path() {
        let start = |id: &str, parent: &str| {
            ChildReport::new(&wire::ChildWorkReport {
                change: wire::ChildWorkChange::Start.into(),
                id: id.to_owned(),
                parent_id: parent.to_owned(),
                state: wire::AgentState::Working.into(),
                ..wire::ChildWorkReport::default()
            })
        };

        let child = start("agent-1", "").unwrap();
        assert_eq!(child.change, ChildChange::Start { parent: None });
        assert!(start("a/b", "").is_err());
        assert!(start("", "").is_err());
        assert!(start(&"a".repeat(129), "").is_err());
        assert!(start("b", "a/c").is_err());
        let unspecified = wire::ChildWorkReport {
            id: "a".to_owned(),
            state: wire::AgentState::Working.into(),
            ..wire::ChildWorkReport::default()
        };
        assert!(ChildReport::new(&unspecified).is_err());
    }
}
