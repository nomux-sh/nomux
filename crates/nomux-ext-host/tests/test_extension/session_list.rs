//! The result of the host service `sessions.list` of the nomux daemon, as an
//! extension reads it: the fields of `SessionListResponse` of the control
//! protocol that the test extension uses.

/// The sessions of the host.
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct SessionList {
    #[prost(message, repeated, tag = "1")]
    pub(crate) sessions: Vec<ListedSession>,
}

/// One session of [`SessionList`].
#[derive(Clone, PartialEq, prost::Message)]
pub(crate) struct ListedSession {
    #[prost(string, tag = "1")]
    pub(crate) id: String,
}
