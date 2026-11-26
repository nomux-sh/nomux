//! The JSON form of the report of a hook command. `docs/cli.md` has the
//! schema.

use std::path::Path;

use serde::Serialize;

use super::hooks_human::state_name;
use crate::hook_commands::{HookToolStatus, HooksAction, HooksReport};

/// Each variant is one top-level field of the document.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HooksBody<'a> {
    HooksChange(ChangeDocument<'a>),
    Hooks(Vec<ToolDocument<'a>>),
}

impl<'a> HooksBody<'a> {
    /// `None` for the answer of a hook: it goes to the tool as it is.
    pub(super) fn of(report: &'a HooksReport) -> Option<Self> {
        Some(match report {
            HooksReport::Answered { .. } => return None,
            HooksReport::Changed {
                action,
                tool,
                path,
                diff,
                applied,
                restores_original,
            } => Self::HooksChange(ChangeDocument {
                action: match action {
                    HooksAction::Install => "install",
                    HooksAction::Uninstall => "uninstall",
                },
                tool,
                path,
                diff,
                changed: !diff.is_empty(),
                applied: *applied,
                restores_original: *action == HooksAction::Uninstall && *restores_original,
            }),
            HooksReport::Statuses(statuses) => {
                Self::Hooks(statuses.iter().map(ToolDocument::from).collect())
            }
        })
    }
}

#[derive(Serialize)]
pub(super) struct ChangeDocument<'a> {
    action: &'static str,
    tool: &'a str,
    path: &'a Path,
    diff: &'a str,
    changed: bool,
    applied: bool,
    restores_original: bool,
}

#[derive(Serialize)]
pub(super) struct ToolDocument<'a> {
    tool: &'a str,
    adapter: Option<&'a str>,
    state: Option<&'static str>,
    path: Option<&'a Path>,
    recorded: bool,
    problem: Option<&'a str>,
}

impl<'a> From<&'a HookToolStatus> for ToolDocument<'a> {
    fn from(status: &'a HookToolStatus) -> Self {
        let hooks = status.hooks.as_ref().ok();
        Self {
            tool: &status.tool,
            adapter: status.adapter.as_deref(),
            state: hooks.map(|hooks| state_name(hooks.state)),
            path: hooks.map(|hooks| hooks.path.as_path()),
            recorded: hooks.is_some_and(|hooks| hooks.recorded),
            problem: status.hooks.as_ref().err().map(String::as_str),
        }
    }
}

/// The documents are a contract with scripts. Each test has the full text,
/// thus a changed field name or a removed field fails it.
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nomux_client::{HooksState, ToolHooks};

    use super::super::json;
    use crate::hook_commands::{HookToolStatus, HooksAction, HooksReport};
    use crate::report::Report;

    #[test]
    fn hooks_change_and_hooks_documents() {
        let change = Report::Hooks(HooksReport::Changed {
            action: HooksAction::Uninstall,
            tool: "testtool".to_owned(),
            path: PathBuf::from("/h/.testtool/settings.json"),
            diff: "-x\n".to_owned(),
            applied: true,
            restores_original: true,
        });
        assert_eq!(
            json::report(&change),
            concat!(
                r#"{"schema_version":1,"hooks_change":{"action":"uninstall","tool":"testtool","#,
                r#""path":"/h/.testtool/settings.json","diff":"-x\n","changed":true,"#,
                r#""applied":true,"restores_original":true}}"#,
                "\n"
            )
        );

        let statuses = Report::Hooks(HooksReport::Statuses(vec![
            HookToolStatus {
                tool: "testtool".to_owned(),
                adapter: Some("nomux.test-adapter".to_owned()),
                hooks: Ok(ToolHooks {
                    path: PathBuf::from("/h/s.json"),
                    state: HooksState::Different,
                    recorded: true,
                }),
            },
            HookToolStatus {
                tool: "old".to_owned(),
                adapter: None,
                hooks: Err("no adapter and no install record".to_owned()),
            },
        ]));
        assert_eq!(
            json::report(&statuses),
            concat!(
                r#"{"schema_version":1,"hooks":[{"tool":"testtool","adapter":"nomux.test-adapter","#,
                r#""state":"different","path":"/h/s.json","recorded":true,"problem":null},"#,
                r#"{"tool":"old","adapter":null,"state":null,"path":null,"recorded":false,"#,
                r#""problem":"no adapter and no install record"}]}"#,
                "\n"
            )
        );
        // The answer of a hook is not a document.
        let answered = Report::Hooks(HooksReport::Answered { output: Vec::new() });
        assert_eq!(json::report(&answered), "");
    }
}
