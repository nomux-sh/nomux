//! The hook commands: `hook`, which an agent tool calls from its hooks, and
//! `hooks install`, `hooks uninstall` and `hooks status`, which change and
//! show the hooks in the settings file of a tool on this computer.
//! `docs/adapters.md` describes the adapters of the tools.

mod arguments;
mod hook_event;
mod report;
mod settings_commands;
mod standard_input;

use nomux_client::ClientError;

pub(crate) use arguments::HooksCommand;
pub(crate) use report::{HookToolStatus, HooksAction, HooksReport};

use crate::report::Report;

/// `hook <tool> <event>`. It never fails: a hook must not break its tool.
pub(crate) async fn hook(tool: String, event: String) -> Report {
    hook_event::send(tool, event).await.into()
}

/// `hooks install`, `hooks uninstall` and `hooks status`. The settings file
/// of a tool is on the computer of the tool, thus the commands work only on
/// the local host.
pub(crate) async fn run(
    command: HooksCommand,
    host_name: Option<&str>,
) -> Result<Report, ClientError> {
    if host_name.is_some() {
        return Err(ClientError::Failed(
            "the --host option cannot be used with a `hooks` command: the settings file of a \
             tool is on its own computer, thus run the command there"
                .to_owned(),
        ));
    }
    let report = match command {
        HooksCommand::Install { tool, apply, .. } => {
            settings_commands::install(&tool, apply).await?
        }
        HooksCommand::Uninstall { tool, apply, .. } => settings_commands::uninstall(&tool, apply)?,
        HooksCommand::Status => settings_commands::status().await?,
    };
    Ok(report.into())
}
