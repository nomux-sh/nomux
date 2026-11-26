//! The text form of the report of a hook command.

use nomux_client::HooksState;

use super::table::{Cell, Table};
use crate::hook_commands::{HookToolStatus, HooksAction, HooksReport};

/// The mark of an empty cell of a table.
const NONE: &str = "-";

pub(super) fn report(report: &HooksReport, width: Option<usize>) -> String {
    match report {
        HooksReport::Answered { .. } => String::new(),
        HooksReport::Changed {
            action,
            tool,
            path,
            diff,
            applied,
            restores_original,
        } => {
            let path = path.display();
            let outcome = match (action, diff.is_empty(), applied) {
                (HooksAction::Install, true, _) => {
                    format!("the hooks of {tool} are installed in {path}: nothing to change")
                }
                (HooksAction::Uninstall, true, true) => {
                    format!("the hooks of {tool} are not in {path}: the install record is removed")
                }
                (HooksAction::Uninstall, true, false) => {
                    format!("the hooks of {tool} are not in {path}: nothing to change")
                }
                (_, false, false) => "dry run: nothing is written. Add --apply to write the \
                                      change"
                    .to_owned(),
                (HooksAction::Install, false, true) => {
                    format!("the hooks of {tool} are installed in {path}")
                }
                (HooksAction::Uninstall, false, true) if *restores_original => {
                    format!("{path} has its content from before the install again")
                }
                (HooksAction::Uninstall, false, true) => format!(
                    "{path} changed after the install: only the entries of nomux are removed"
                ),
            };
            format!("{diff}{outcome}\n")
        }
        HooksReport::Statuses(statuses) if statuses.is_empty() => {
            "no tool has an adapter or installed hooks\n".to_owned()
        }
        HooksReport::Statuses(statuses) => status_table(statuses, width),
    }
}

/// Only what a script needs: the tools whose hooks are not installed.
pub(super) fn quiet_report(report: &HooksReport) -> String {
    match report {
        HooksReport::Statuses(statuses) => statuses
            .iter()
            .filter(|status| !matches!(&status.hooks, Ok(hooks) if hooks.state == HooksState::Installed))
            .flat_map(|status| [status.tool.as_str(), "\n"])
            .collect(),
        HooksReport::Answered { .. } | HooksReport::Changed { .. } => String::new(),
    }
}

fn status_table(statuses: &[HookToolStatus], width: Option<usize>) -> String {
    const ADAPTER: usize = 1;
    const HOOKS: usize = 2;
    let mut table = Table::new(&["TOOL", "ADAPTER", "HOOKS"]).shorten(&[HOOKS, ADAPTER]);
    for status in statuses {
        let hooks = match &status.hooks {
            Ok(hooks) => format!("{} in {}", state_name(hooks.state), hooks.path.display()),
            Err(problem) => format!("not known: {problem}"),
        };
        table.push(vec![
            Cell::new(&status.tool),
            Cell::new(status.adapter.as_deref().unwrap_or(NONE)),
            Cell::new(hooks),
        ]);
    }
    table.render(width)
}

pub(super) fn state_name(state: HooksState) -> &'static str {
    match state {
        HooksState::Installed => "installed",
        HooksState::Missing => "missing",
        HooksState::Different => "different",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nomux_client::ToolHooks;

    use super::super::table::assert_fits;
    use super::*;

    #[test]
    fn the_table_fits_a_terminal_of_60_columns() {
        let statuses = [HookToolStatus {
            tool: "claude-code".to_owned(),
            adapter: Some("nomux.adapter-claude-code-with-a-long-name".to_owned()),
            hooks: Ok(ToolHooks {
                path: PathBuf::from("/home/user/.claude/settings.json"),
                state: HooksState::Installed,
                recorded: true,
            }),
        }];

        let text = status_table(&statuses, Some(60));

        assert_fits(&text, 60);
        assert!(text.contains("claude-code"), "{text}");
    }
}
