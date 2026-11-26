//! The work of the worktrees that no request starts: the removal at the end
//! of the grace time, the rules of the settings, and the look at the free
//! space every 5 minutes while a client has a subscription.
//!
//! A daemon with an empty trash place, no rule and no subscription has no
//! timer: each loop waits for a notification.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use nomux_core::configuration::AutoClean;
use tokio::time::sleep;
use tracing::debug;

use super::plan::{self, Action, Selection};
use super::{Sizes, Sources, disk};
use crate::clock::now;
use crate::handlers::Context;

/// The time between two looks at the free space while a client has a
/// subscription.
const DISK_LOOK_INTERVAL: Duration = Duration::from_mins(5);

/// Runs until the daemon stops.
pub(crate) async fn run(context: Arc<Context>) {
    tokio::join!(
        grace_time(&context),
        rules_of_the_settings(&context),
        disk_looks(&context),
    );
}

fn sources(context: &Context) -> Sources<'_> {
    Sources {
        workspace: &context.workspace,
        sessions: &context.sessions,
        extensions: &context.extensions,
    }
}

/// Removes each trash entry at the end of its grace time.
async fn grace_time(context: &Context) {
    let worktrees = &context.worktrees;
    let mut failed = HashSet::new();
    loop {
        match worktrees.next_expiry(&failed) {
            None => worktrees.trash_changed.notified().await,
            Some(expires_at_ms) => {
                let wait = Duration::from_millis(expires_at_ms.saturating_sub(now().0));
                tokio::select! {
                    () = worktrees.trash_changed.notified() => continue,
                    () = sleep(wait) => {}
                }
                let _cleaning = worktrees.cleaning.lock().await;
                worktrees
                    .remove_expired(&sources(context), &mut failed)
                    .await;
            }
        }
    }
}

/// Runs the rules of `auto` of the settings at the start of the daemon and
/// after each notification.
async fn rules_of_the_settings(context: &Context) {
    let worktrees = &context.worktrees;
    if worktrees.settings.auto.is_empty() {
        return;
    }
    loop {
        for rule in &worktrees.settings.auto {
            let action = match rule {
                AutoClean::BuildOutputOfFinished => Action::BuildOutputOfFinished,
                AutoClean::Finished => Action::Finished,
                AutoClean::StaleBuildOutput => Action::Stale,
            };
            let selection = Selection::automatic(action);
            let Ok(projects) = worktrees
                .snapshot("", &sources(context), Sizes::Cached)
                .await
            else {
                continue;
            };
            let items: Vec<_> = plan::items(&projects, &selection)
                .into_iter()
                .filter(|item| item.refusals.is_empty())
                .collect();
            if items.is_empty() {
                continue;
            }
            let _cleaning = worktrees.cleaning.lock().await;
            let id = worktrees.plans.keep(selection, items);
            if let Some(plan) = worktrees.plans.get(&id) {
                let response = worktrees.run(plan, &sources(context)).await;
                debug!(
                    ?rule,
                    results = response.results.len(),
                    "a rule of the settings ran"
                );
            }
        }
        worktrees.auto_wanted.notified().await;
    }
}

/// Reads the free space every 5 minutes while a client has a subscription.
async fn disk_looks(context: &Context) {
    let worktrees = &context.worktrees;
    loop {
        worktrees.subscribed.notified().await;
        loop {
            if let Ok(projects) = context.workspace.projects("") {
                disk::check(worktrees, &projects);
            }
            sleep(DISK_LOOK_INTERVAL).await;
            if !context.events.has_subscriptions() {
                break;
            }
        }
    }
}
