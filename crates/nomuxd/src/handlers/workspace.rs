//! The handlers of the Workspace service.

use nomux_proto::wire::v1::{
    Error, WorkspaceAddProjectRequest, WorkspaceCreateGroupRequest, WorkspaceMoveRequest,
    WorkspaceMoveResponse, WorkspaceRemoveGroupRequest, WorkspaceRemoveGroupResponse,
    WorkspaceRemoveProjectRequest, WorkspaceRemoveProjectResponse, WorkspaceRenameRequest,
    WorkspaceRenameResponse, response,
};

use super::Context;

pub(super) fn tree(context: &Context) -> response::Result {
    response::Result::WorkspaceTree(context.workspace.tree())
}

pub(super) fn add_project(
    request: WorkspaceAddProjectRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let added = context.workspace.add_project(request)?;
    Ok(response::Result::WorkspaceAddProject(added))
}

pub(super) fn create_group(
    request: WorkspaceCreateGroupRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let created = context.workspace.create_group(request)?;
    Ok(response::Result::WorkspaceCreateGroup(created))
}

pub(super) fn remove_project(
    request: &WorkspaceRemoveProjectRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let revision = context.workspace.remove(&request.id, false)?;
    Ok(response::Result::WorkspaceRemoveProject(
        WorkspaceRemoveProjectResponse { revision },
    ))
}

pub(super) fn remove_group(
    request: &WorkspaceRemoveGroupRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let revision = context.workspace.remove(&request.id, true)?;
    Ok(response::Result::WorkspaceRemoveGroup(
        WorkspaceRemoveGroupResponse { revision },
    ))
}

pub(super) fn rename(
    request: &WorkspaceRenameRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let revision = context.workspace.rename(request)?;
    Ok(response::Result::WorkspaceRename(WorkspaceRenameResponse {
        revision,
    }))
}

pub(super) fn move_node(
    request: &WorkspaceMoveRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let revision = context.workspace.move_node(request)?;
    Ok(response::Result::WorkspaceMove(WorkspaceMoveResponse {
        revision,
    }))
}
