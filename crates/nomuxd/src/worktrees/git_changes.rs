//! The calls to the git extension that change git for a clean: the move to
//! the trash place and back, the removal and the prune. The extension
//! checks the facts of the worktree again and runs the `git` of the user.

use nomux_ext_host::ExtensionHost;
use nomux_proto::wire::v1::{
    WorktreeDeleteRequest, WorktreeDeleteResponse, WorktreeMoveFromTrashRequest,
    WorktreeMoveFromTrashResponse, WorktreeMoveToTrashRequest, WorktreeMoveToTrashResponse,
    WorktreePruneRequest, WorktreePruneResponse,
};
use prost::Message;

use super::facts::GIT_EXTENSION;

pub(super) async fn move_to_trash(
    extensions: &ExtensionHost,
    request: WorktreeMoveToTrashRequest,
) -> Result<WorktreeMoveToTrashResponse, String> {
    call(extensions, "worktree_move_to_trash", &request).await
}

pub(super) async fn move_from_trash(
    extensions: &ExtensionHost,
    request: WorktreeMoveFromTrashRequest,
) -> Result<WorktreeMoveFromTrashResponse, String> {
    call(extensions, "worktree_move_from_trash", &request).await
}

pub(super) async fn delete(
    extensions: &ExtensionHost,
    request: WorktreeDeleteRequest,
) -> Result<WorktreeDeleteResponse, String> {
    call(extensions, "worktree_delete", &request).await
}

pub(super) async fn prune(
    extensions: &ExtensionHost,
    request: WorktreePruneRequest,
) -> Result<WorktreePruneResponse, String> {
    call(extensions, "worktree_prune", &request).await
}

/// Calls `method` of the git extension. The error is a text for the user.
async fn call<Response: Message + Default>(
    extensions: &ExtensionHost,
    method: &str,
    request: &impl Message,
) -> Result<Response, String> {
    let payload = extensions
        .call(GIT_EXTENSION, method, request.encode_to_vec())
        .await
        .map_err(|error| format!("the git extension {GIT_EXTENSION} failed `{method}`: {error}"))?;
    Response::decode(payload.as_slice()).map_err(|error| {
        format!(
            "the git extension {GIT_EXTENSION} answered `{method}` with a wrong payload: {error}"
        )
    })
}
