//! The handlers of the Worktree service.

use nomux_proto::wire::v1::{Error, WorktreeListRequest, response};

use super::Context;
use crate::worktrees::Sources;

pub(super) async fn list(
    request: &WorktreeListRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let sources = Sources {
        workspace: &context.workspace,
        sessions: &context.sessions,
        extensions: &context.extensions,
    };
    let listed = context.worktrees.list(request, sources).await?;
    Ok(response::Result::WorktreeList(listed))
}
