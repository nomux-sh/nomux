//! The git facts of the worktrees of a Project, from the method
//! `worktree_facts` of the git extension. The daemon has no git code: the
//! extension reads the repository.

use std::path::Path;

use nomux_ext_host::{CallError, ErrorCode, ExtensionHost};
use nomux_proto::wire::v1::{WorktreeFactsRequest, WorktreeFactsResponse};
use prost::Message;

/// The identifier of the git extension.
pub(super) const GIT_EXTENSION: &str = "nomux.git";
/// The method of the git extension that gives the facts.
const FACTS_METHOD: &str = "worktree_facts";

/// Asks the git extension for the facts of the repository of `directory`,
/// with the base branch `base_branch` of the settings. The error is a text
/// for the user that says why the facts are missing and what the user can
/// do.
pub(super) async fn of_project(
    extensions: &ExtensionHost,
    directory: &Path,
    base_branch: &str,
) -> Result<WorktreeFactsResponse, String> {
    let request = WorktreeFactsRequest {
        repository: directory.display().to_string(),
        base_branch: base_branch.to_owned(),
    };
    let payload = extensions
        .call(GIT_EXTENSION, FACTS_METHOD, request.encode_to_vec())
        .await
        .map_err(|error| unavailable(&error, directory))?;
    WorktreeFactsResponse::decode(payload.as_slice()).map_err(|error| {
        format!(
            "the git extension {GIT_EXTENSION} answered `{FACTS_METHOD}` with a payload that is \
             not a WorktreeFactsResponse: {error}"
        )
    })
}

fn unavailable(error: &CallError, directory: &Path) -> String {
    let directory = directory.display();
    match error {
        CallError::NotInstalled => format!(
            "the git extension {GIT_EXTENSION} is not installed. Install it with `nomux ext \
             install git --grant-all --grant fs={directory}`"
        ),
        CallError::Answered(answer) if answer.code == ErrorCode::PermissionDenied => format!(
            "the git extension {GIT_EXTENSION} may not read {directory}. Approve the directory: \
             `nomux ext rm {GIT_EXTENSION}`, then `nomux ext install git --grant-all --grant \
             fs={directory}` with a --grant for each directory that it had"
        ),
        // The extension protocol names the method in the error of a method that
        // the extension does not have.
        CallError::Answered(answer)
            if answer.code == ErrorCode::NotFound && answer.message.contains(FACTS_METHOD) =>
        {
            format!(
                "the git extension {GIT_EXTENSION} is too old: it has no method \
                 `{FACTS_METHOD}`. Install a newer version: `nomux ext rm {GIT_EXTENSION}`, then \
                 `nomux ext install git --grant-all --grant fs={directory}`"
            )
        }
        CallError::Answered(answer) => format!(
            "the git extension {GIT_EXTENSION} gave no worktrees: {}",
            answer.message
        ),
        error => format!("the git extension {GIT_EXTENSION} gave no worktrees: {error}"),
    }
}
