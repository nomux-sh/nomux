//! The rules that let a later daemon talk to this holder.

use nomux_holder_protocol::wire::{ChildStatus, ErrorCode, Reply, Request};

use crate::fixture::{Client, Holder, runtime_directory};

#[test]
fn a_request_kind_of_a_later_version_gets_unknown_kind_and_the_connection_stays_open() {
    let runtime = runtime_directory();
    let _holder = Holder::start(runtime.path(), "later", &[], &["/bin/sh"]);
    let mut client = Client::connect(runtime.path());

    assert_eq!(
        client.request_frame(0x01F0, b"from a later version"),
        Reply::Error(ErrorCode::UnknownKind)
    );

    assert_eq!(
        client.request(Request::Status),
        Reply::Status(ChildStatus::Running)
    );
}
