//! `hooks install`, `hooks uninstall` and `hooks status`: the hooks of a
//! tool in its settings file on this computer. Without `--apply`, a command
//! writes nothing.

use std::collections::BTreeMap;

use nomux_client::{
    ClientError, Host, SettingsChange, hooks_of_tool, plan_hook_install, plan_hook_uninstall,
    tools_with_installed_hooks,
};
use nomux_core::hook_event::is_tool_name;

use super::report::{HookToolStatus, HooksAction, HooksReport};
use crate::commands::KIND;

pub(super) async fn install(tool: &str, apply: bool) -> Result<HooksReport, ClientError> {
    check_tool_name(tool)?;
    let plan = Host::local()?
        .client(KIND)
        .await?
        .hook_install_plan(tool)
        .await?;
    let change = plan_hook_install(tool, &plan)?;
    changed(HooksAction::Install, tool, change, apply)
}

/// An uninstall needs only the record of the install: no daemon and no
/// adapter.
pub(super) fn uninstall(tool: &str, apply: bool) -> Result<HooksReport, ClientError> {
    check_tool_name(tool)?;
    let change = plan_hook_uninstall(tool)?;
    changed(HooksAction::Uninstall, tool, change, apply)
}

/// Each tool that has an adapter or a record of an install.
pub(super) async fn status() -> Result<HooksReport, ClientError> {
    let mut client = Host::local()?.client(KIND).await?;
    let mut tools: BTreeMap<String, Option<String>> = client
        .hook_adapters()
        .await?
        .into_iter()
        .map(|adapter| (adapter.tool, Some(adapter.extension_id)))
        .collect();
    for tool in tools_with_installed_hooks()? {
        tools.entry(tool).or_insert(None);
    }
    let mut statuses = Vec::new();
    for (tool, adapter) in tools {
        let hooks = match &adapter {
            Some(_) => match client.hook_install_plan(&tool).await {
                Ok(plan) => hooks_of_tool(&tool, Some(&plan)),
                Err(error) => Err(error),
            },
            None => hooks_of_tool(&tool, None),
        };
        let hooks = match hooks {
            Ok(Some(hooks)) => Ok(hooks),
            Ok(None) => Err("no adapter and no install record".to_owned()),
            Err(error) => Err(error.to_string()),
        };
        statuses.push(HookToolStatus {
            tool,
            adapter,
            hooks,
        });
    }
    Ok(HooksReport::Statuses(statuses))
}

fn changed(
    action: HooksAction,
    tool: &str,
    change: SettingsChange,
    apply: bool,
) -> Result<HooksReport, ClientError> {
    let report = HooksReport::Changed {
        action,
        tool: tool.to_owned(),
        path: change.path.clone(),
        diff: change.diff.clone(),
        applied: apply,
        restores_original: change.restores_original,
    };
    if apply {
        change.apply(tool)?;
    }
    Ok(report)
}

/// The name of a tool is a part of the path of its install record.
fn check_tool_name(tool: &str) -> Result<(), ClientError> {
    if is_tool_name(tool) {
        return Ok(());
    }
    Err(ClientError::Failed(format!(
        "`{tool}` is not a tool name: a lower-case letter, then lower-case letters, digits and `-`"
    )))
}
