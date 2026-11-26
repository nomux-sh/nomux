//! The handlers of the Worktree service.

use nomux_proto::wire::v1::{
    Error, WorktreeCleanRequest, WorktreeEmptyTrashRequest, WorktreeListRequest,
    WorktreeMarkRequest, WorktreePlanRequest, WorktreeRestoreRequest, WorktreeTrashRequest,
    response,
};

use super::Context;
use crate::worktrees::Sources;

fn sources(context: &Context) -> Sources<'_> {
    Sources {
        workspace: &context.workspace,
        sessions: &context.sessions,
        extensions: &context.extensions,
    }
}

pub(super) async fn list(
    request: &WorktreeListRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let listed = context.worktrees.list(request, sources(context)).await?;
    Ok(response::Result::WorktreeList(listed))
}

pub(super) async fn plan(
    request: &WorktreePlanRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let plan = context.worktrees.plan(request, sources(context)).await?;
    Ok(response::Result::WorktreePlan(plan))
}

pub(super) async fn clean(
    request: &WorktreeCleanRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let cleaned = context.worktrees.clean(request, sources(context)).await?;
    Ok(response::Result::WorktreeClean(cleaned))
}

pub(super) async fn mark(
    request: &WorktreeMarkRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let marked = context.worktrees.mark(request, sources(context)).await?;
    Ok(response::Result::WorktreeMark(marked))
}

pub(super) fn trash(
    request: &WorktreeTrashRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let entries = context.worktrees.trash(request, &sources(context))?;
    Ok(response::Result::WorktreeTrash(entries))
}

pub(super) async fn restore(
    request: &WorktreeRestoreRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let restored = context
        .worktrees
        .restore(request, &sources(context))
        .await?;
    Ok(response::Result::WorktreeRestore(restored))
}

pub(super) async fn empty_trash(
    request: &WorktreeEmptyTrashRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let emptied = context
        .worktrees
        .empty_trash(request, &sources(context))
        .await?;
    Ok(response::Result::WorktreeEmptyTrash(emptied))
}
