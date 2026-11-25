//! The host services, as the embedder of the host supplies them.

use std::future::Future;
use std::pin::Pin;

/// The error of one call: a code of the extension protocol and a message.
///
/// A host service answers with it, and a method of an extension answers with
/// it.
pub use nomux_extension::CallError as ServiceError;

/// The result of a host service: the payload of the result, or an error.
pub type ServiceReply = Pin<Box<dyn Future<Output = Result<Vec<u8>, ServiceError>> + Send>>;

/// The services that the extensions can call.
///
/// The embedder (the nomux daemon) implements this trait. The host calls
/// [`HostServices::call`] only for a method that the embedder offers and for
/// which the extension has the scope:
///
/// - If [`HostServices::required_scope`] is `None`, the extension gets
///   `ERROR_CODE_NOT_FOUND`.
/// - If the user did not approve the scope, the extension gets
///   `ERROR_CODE_PERMISSION_DENIED`.
pub trait HostServices: Send + Sync + 'static {
    /// The scope that an extension needs to call `method`, for example
    /// `sessions:read` for `sessions.list`. `None` if the embedder does not
    /// offer the method.
    fn required_scope(&self, method: &str) -> Option<String>;

    /// Does one call for the extension `extension_id`.
    ///
    /// The host drops the result when the extension cancels the call or
    /// when its process ends.
    fn call(&self, extension_id: &str, method: &str, payload: Vec<u8>) -> ServiceReply;
}
