//! `worktree clean`: the plan, then one question on a terminal or `--yes`,
//! then the clean of exactly the items of that plan. Without `--yes` and
//! without a terminal, the command shows the plan and changes nothing.

use std::io::{self, BufRead, IsTerminal, Write};

use nomux_client::{CleanAction, CleanRequest, Client, ClientError};

use super::arguments::CleanArguments;
use super::{WorktreeReport, fetch_projects, selected_project, worktree_path};
use crate::output::{plan_text, short_size};
use crate::report::Report;

pub(super) async fn run(
    client: &mut Client,
    arguments: &CleanArguments,
    on_remote_host: bool,
) -> Result<Report, ClientError> {
    let workspace = client.workspace().await?;
    let project_id = selected_project(&workspace, &arguments.projects, on_remote_host)?;
    if arguments.fetch {
        fetch_projects(&workspace, project_id.as_deref(), on_remote_host)?;
    }
    let mut worktree_paths = Vec::new();
    for worktree in &arguments.worktrees {
        worktree_paths.push(worktree_path(client, worktree).await?);
    }
    let request = CleanRequest {
        project_id,
        action: action(arguments),
        worktree_paths,
        delete_branch: arguments.delete_branch,
        allow_unpushed: arguments.allow_unpushed,
        include_ignored: arguments.include_ignored,
        now: arguments.now,
    };
    let plan = client.plan_clean(&request).await?;
    let runnable: Vec<_> = plan
        .items
        .iter()
        .filter(|item| item.refusals.is_empty())
        .collect();
    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    if arguments.dry_run || runnable.is_empty() || !(arguments.yes || interactive) {
        return Ok(Report::Worktree(WorktreeReport::Plan(plan)));
    }
    if !arguments.yes {
        eprint!("{}", plan_text(&plan, None));
        confirm(&format!(
            "Clean {} item(s), {} (upper bound)?",
            runnable.len(),
            short_size(plan.total_bytes)
        ))?;
    }
    let cleaned = client.clean_worktrees(&plan.plan_id).await?;
    Ok(Report::Worktree(WorktreeReport::Cleaned { plan, cleaned }))
}

fn action(arguments: &CleanArguments) -> CleanAction {
    if arguments.build_output {
        CleanAction::BuildOutput
    } else if arguments.stale {
        CleanAction::Stale
    } else if arguments.prune {
        CleanAction::Prune
    } else {
        CleanAction::Finished
    }
}

/// Asks `question` on the terminal. Each answer but `y` or `yes` is an
/// error: nothing changes.
pub(super) fn confirm(question: &str) -> Result<(), ClientError> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(ClientError::Failed(
            "the command needs an answer, and the standard input is not a terminal. Nothing \
             changed. Add --yes"
                .to_owned(),
        ));
    }
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "{question} [y/N] ");
    let _ = stderr.flush();
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|error| ClientError::Failed(format!("the answer cannot be read: {error}")))?;
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(ClientError::Failed(
            "nothing changed: the answer was not yes".to_owned(),
        ))
    }
}
