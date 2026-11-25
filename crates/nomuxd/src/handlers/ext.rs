//! The handlers of the Ext service.

use std::path::PathBuf;

use nomux_ext_host::{
    CallError, Enablement, ExtensionHost, InstallError, InstalledExtension, Permissions, RunState,
    StateError,
};
use nomux_proto::wire::v1::{
    Error, ErrorCode, ExtCallError, ExtCallRequest, ExtCallResponse, ExtInspectRequest,
    ExtInspectResponse, ExtInstallRequest, ExtInstallResponse, ExtListResponse, ExtRemoveRequest,
    ExtRemoveResponse, ExtSetEnabledRequest, ExtSetEnabledResponse, Extension,
    ExtensionPermissions, ExtensionState, MAX_ENVELOPE_LENGTH, ext_call_response, response,
};
use tokio::task::spawn_blocking;

use super::Context;

const DISABLED_BY_USER: &str = "the user disabled the extension";

/// The bytes of an envelope that are not the result of a request: the
/// request number and the lengths of the messages around the result.
const ENVELOPE_OVERHEAD: usize = 64;

pub(super) fn list(context: &Context) -> response::Result {
    let statuses = context.extensions.extensions();
    let extensions = statuses
        .iter()
        .map(|status| extension(&status.extension, &status.run_state));
    response::Result::ExtList(ExtListResponse {
        extensions: extensions.collect(),
    })
}

pub(super) async fn inspect(request: ExtInspectRequest) -> Result<response::Result, Error> {
    let directory = package_directory(request.directory)?;
    let manifest = blocking(move || ExtensionHost::inspect(&directory))
        .await?
        .map_err(|error| install_failure(&error))?;
    let requested = ExtensionPermissions {
        fs: manifest.permissions.fs,
        process: manifest.permissions.process,
        network: manifest.permissions.network,
        scopes: manifest.permissions.scopes,
    };
    Ok(response::Result::ExtInspect(ExtInspectResponse {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version.to_string(),
        requested: Some(requested),
    }))
}

pub(super) async fn install(
    request: ExtInstallRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let directory = package_directory(request.directory)?;
    // A request without the field approves no permission.
    let granted = request.granted.unwrap_or_default();
    let granted = Permissions {
        fs: granted.fs,
        process: granted.process,
        network: granted.network,
        scopes: granted.scopes,
    };
    let host = context.extensions.clone();
    let installed = blocking(move || host.install(&directory, granted))
        .await?
        .map_err(|error| install_failure(&error))?;
    Ok(response::Result::ExtInstall(ExtInstallResponse {
        extension: Some(extension(&installed, &RunState::Stopped)),
    }))
}

pub(super) async fn remove(
    request: &ExtRemoveRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let removed = context.extensions.remove(&request.id).await;
    removed.map_err(|error| state_failure(&request.id, &error))?;
    Ok(response::Result::ExtRemove(ExtRemoveResponse {}))
}

pub(super) async fn set_enabled(
    request: &ExtSetEnabledRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let host = &context.extensions;
    let id = &request.id;
    let changed = if request.enabled {
        host.enable(id).await
    } else {
        host.disable(id, DISABLED_BY_USER).await
    };
    changed.map_err(|error| state_failure(id, &error))?;
    let status = host.extension(id).ok_or_else(|| not_installed(id))?;
    Ok(response::Result::ExtSetEnabled(ExtSetEnabledResponse {
        extension: Some(extension(&status.extension, &status.run_state)),
    }))
}

pub(super) async fn call(
    request: ExtCallRequest,
    context: &Context,
) -> Result<response::Result, Error> {
    let called = context
        .extensions
        .call(&request.ext, &request.method, request.payload);
    let answer = match called.await {
        Ok(payload) => ext_call_response::Answer::Payload(payload),
        Err(CallError::Answered(error)) => ext_call_response::Answer::Error(ExtCallError {
            // The two enumerations have the same values.
            code: i32::from(error.code),
            message: error.message,
        }),
        Err(error) => return Err(call_failure(&request.ext, &error)),
    };
    let result = response::Result::ExtCall(ExtCallResponse {
        answer: Some(answer),
    });
    // A frame of an extension can be larger than an envelope.
    let length = result.encoded_len();
    if length > MAX_ENVELOPE_LENGTH - ENVELOPE_OVERHEAD {
        return Err(failed(format!(
            "the answer of the extension has {length} bytes, which do not go into one envelope \
             of {MAX_ENVELOPE_LENGTH} bytes"
        )));
    }
    Ok(result)
}

/// The directory of a package, as a request names it.
fn package_directory(directory: String) -> Result<PathBuf, Error> {
    let directory = PathBuf::from(directory);
    if directory.is_absolute() {
        return Ok(directory);
    }
    Err(failed(format!(
        "the package directory `{}` is not an absolute path",
        directory.display()
    )))
}

/// Runs work that reads or writes files on a thread that is not the thread
/// of the daemon: a slow disk must not delay a session.
async fn blocking<Output: Send + 'static>(
    work: impl FnOnce() -> Output + Send + 'static,
) -> Result<Output, Error> {
    spawn_blocking(work)
        .await
        .map_err(|error| failed(format!("the work on the package did not complete: {error}")))
}

fn extension(installed: &InstalledExtension, run_state: &RunState) -> Extension {
    let (state, state_reason) = state(&installed.enablement, run_state);
    let granted = &installed.granted;
    Extension {
        id: installed.manifest.id.clone(),
        name: installed.manifest.name.clone(),
        version: installed.manifest.version.to_string(),
        state: state.into(),
        state_reason,
        granted: Some(ExtensionPermissions {
            fs: granted.fs.clone(),
            process: granted.process.clone(),
            network: granted.network,
            scopes: granted.scopes.clone(),
        }),
    }
}

/// The state of an extension and the reason for it. An extension that is
/// disabled has no other state: the host does not start it.
fn state(enablement: &Enablement, run_state: &RunState) -> (ExtensionState, String) {
    if let Enablement::Disabled { reason } = enablement {
        return (ExtensionState::Disabled, reason.clone());
    }
    match run_state {
        RunState::Stopped => (ExtensionState::Stopped, String::new()),
        RunState::Starting => (ExtensionState::Starting, String::new()),
        RunState::Running => (ExtensionState::Running, String::new()),
        RunState::Failed { failure, .. } => (ExtensionState::Failed, failure.to_string()),
    }
}

fn install_failure(error: &InstallError) -> Error {
    failed(error.to_string())
}

fn state_failure(id: &str, error: &StateError) -> Error {
    match error {
        StateError::NotInstalled => not_installed(id),
        StateError::Io(_) => failed(format!("the extension {id}: {error}")),
    }
}

/// The failure of a call that the extension did not answer.
fn call_failure(id: &str, error: &CallError) -> Error {
    let code = match error {
        CallError::NotInstalled => return not_installed(id),
        CallError::Timeout => ErrorCode::Timeout,
        _ => ErrorCode::Failed,
    };
    Error {
        code: code.into(),
        message: format!("the extension {id}: {error}"),
    }
}

fn not_installed(id: &str) -> Error {
    Error {
        code: ErrorCode::NotFound.into(),
        message: format!("no extension has the identifier {id}"),
    }
}

fn failed(message: String) -> Error {
    Error {
        code: ErrorCode::Failed.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use nomux_ext_host::{ErrorCode as ExtensionErrorCode, Failure};
    use nomux_proto::wire::v1::ExtCallErrorCode;

    use super::*;

    #[test]
    fn a_disabled_extension_shows_why_and_not_the_state_of_its_last_process() {
        let failed = RunState::Failed {
            failure: Failure::StartTimeout,
            retry_at: None,
        };
        let disabled = Enablement::Disabled {
            reason: "the extension failed: it did not start".to_owned(),
        };

        assert_eq!(
            state(&disabled, &failed),
            (
                ExtensionState::Disabled,
                "the extension failed: it did not start".to_owned()
            )
        );
        assert_eq!(
            state(&Enablement::Enabled, &failed),
            (
                ExtensionState::Failed,
                "the extension did not start in time".to_owned()
            )
        );
        assert_eq!(
            state(&Enablement::Enabled, &RunState::Running),
            (ExtensionState::Running, String::new())
        );
    }

    #[test]
    fn the_error_code_of_an_extension_has_the_same_value_in_the_control_protocol() {
        let pairs = [
            (ExtensionErrorCode::Failed, ExtCallErrorCode::Failed),
            (ExtensionErrorCode::NotFound, ExtCallErrorCode::NotFound),
            (ExtensionErrorCode::Timeout, ExtCallErrorCode::Timeout),
            (
                ExtensionErrorCode::PermissionDenied,
                ExtCallErrorCode::PermissionDenied,
            ),
            (
                ExtensionErrorCode::VersionIncompatible,
                ExtCallErrorCode::VersionIncompatible,
            ),
            (
                ExtensionErrorCode::InvalidArgument,
                ExtCallErrorCode::InvalidArgument,
            ),
            (ExtensionErrorCode::Cancelled, ExtCallErrorCode::Cancelled),
        ];
        for (extension, control) in pairs {
            assert_eq!(i32::from(extension), i32::from(control));
        }
    }

    #[test]
    fn only_a_call_without_an_answer_in_time_has_the_timeout_code() {
        let code = |error: &CallError| call_failure("nomux.git", error).code;

        assert_eq!(code(&CallError::NotInstalled), ErrorCode::NotFound as i32);
        assert_eq!(code(&CallError::Timeout), ErrorCode::Timeout as i32);
        assert_eq!(code(&CallError::Stopped), ErrorCode::Failed as i32);
        assert_eq!(
            code(&CallError::Failed(Failure::HealthTimeout)),
            ErrorCode::Failed as i32
        );
    }
}
