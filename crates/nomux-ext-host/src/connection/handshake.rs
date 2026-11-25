//! The host side of the handshake: `Hello` and `Welcome`.

use nomux_extension::wire::v1::envelope::Kind;
use nomux_extension::wire::v1::request::Body;
use nomux_extension::wire::v1::{Envelope, ErrorCode, Hello, Response, Welcome, response};
use nomux_extension::{ApiRequirement, ApiVersion, Permissions};

use super::envelopes;
use super::extension_process::{ExtensionProcess, ProcessEvent};
use crate::failure::Failure;

/// What the host knows about an extension before it starts the process.
#[derive(Debug)]
pub(crate) struct Identity {
    /// The identifier in the manifest.
    pub(crate) extension_id: String,
    /// The extension API versions that the manifest names.
    pub(crate) api: ApiRequirement,
    /// The permissions that the user approved.
    pub(crate) granted: Permissions,
}

/// Sends `Hello` and examines the first message of the extension.
pub(super) async fn greet(
    process: &mut ExtensionProcess,
    request_id: u64,
    identity: &Identity,
) -> Result<(), Failure> {
    let hello = Hello {
        api_version: Some(ApiVersion::CURRENT),
        extension_id: identity.extension_id.clone(),
        permissions: Some(identity.granted.clone()),
    };
    let request = envelopes::request(request_id, Body::Hello(hello));
    process.send(&request).await?;
    match process.next().await? {
        ProcessEvent::Exited(status) => Err(Failure::Exited(status)),
        ProcessEvent::Envelope(envelope) => match answer_to_hello(envelope, request_id)? {
            response::Result::Welcome(welcome) => examine(&welcome, identity),
            response::Result::Error(error)
                if error.code == i32::from(ErrorCode::VersionIncompatible) =>
            {
                Err(Failure::VersionIncompatible(error.message))
            }
            response::Result::Error(error) => Err(Failure::Protocol(format!(
                "the extension refused `Hello`: {}",
                error.message
            ))),
            _ => Err(not_welcome()),
        },
    }
}

fn answer_to_hello(envelope: Envelope, request_id: u64) -> Result<response::Result, Failure> {
    match envelope.kind {
        Some(Kind::Response(Response {
            id,
            result: Some(result),
        })) if id == request_id => Ok(result),
        _ => Err(not_welcome()),
    }
}

fn not_welcome() -> Failure {
    Failure::Protocol("the first message is not the response to `Hello`".to_owned())
}

/// The program must be the program of the manifest: the same identifier,
/// and an extension API version that the manifest names.
fn examine(welcome: &Welcome, identity: &Identity) -> Result<(), Failure> {
    if welcome.extension_id != identity.extension_id {
        return Err(Failure::IdentityMismatch {
            claimed: welcome.extension_id.clone(),
        });
    }
    let built_for = welcome.api_version.unwrap_or_default();
    if !identity.api.matches(built_for) {
        return Err(Failure::VersionIncompatible(format!(
            "the program was built for the extension API version {built_for}, but its manifest names {}",
            identity.api
        )));
    }
    Ok(())
}
