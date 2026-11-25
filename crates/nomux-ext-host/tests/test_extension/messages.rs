//! The messages of the methods of the test extension. The extension and the
//! tests of the host use the same file.

/// The argument and the result of most methods.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct Text {
    #[prost(string, tag = "1")]
    pub(crate) text: String,
}

/// The result of the `process` method: what the process of the extension
/// got from the host.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct ProcessInfo {
    #[prost(uint32, tag = "1")]
    pub(crate) pid: u32,
    #[prost(string, tag = "2")]
    pub(crate) working_directory: String,
    /// The names of the environment variables.
    #[prost(string, repeated, tag = "3")]
    pub(crate) variables: Vec<String>,
    /// The scopes in `Hello`.
    #[prost(string, repeated, tag = "4")]
    pub(crate) scopes: Vec<String>,
}
