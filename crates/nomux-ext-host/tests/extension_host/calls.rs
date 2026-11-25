//! Calls in the two directions, their errors, their time limits and their
//! cancellation, and the events of an extension.

use std::time::Duration;

use nomux_ext_host::{CallError, ErrorCode, ExtensionEvent, Permissions, RunState};
use prost::Message;
use tokio::sync::broadcast;

use crate::fixture::{Fixture, HEALTHY, SESSIONS_READ, within};
use crate::messages::{ProcessInfo, Text};

/// Waits for the event of the `slow` method that says that the extension
/// saw the cancellation of its call.
async fn cancellation_seen(events: &mut broadcast::Receiver<ExtensionEvent>) {
    let event = within(events.recv()).await;
    assert_eq!(event.map(|event| event.name).as_deref(), Ok("cancel-seen"));
}

#[tokio::test]
async fn a_call_gives_the_result_and_an_event_names_its_extension() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);
    let mut events = fixture.host.subscribe();

    let answer = fixture.text(HEALTHY, "echo", "hello").await;

    assert_eq!(answer.as_deref(), Ok("hello"));
    let text = Text {
        text: "hello".to_owned(),
    };
    let expected = ExtensionEvent {
        extension_id: HEALTHY.to_owned(),
        name: "echoed".to_owned(),
        payload: text.encode_to_vec(),
    };
    assert_eq!(within(events.recv()).await, Ok(expected));
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn an_error_of_the_extension_reaches_the_caller_with_its_code() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);

    let failed = fixture.text(HEALTHY, "fail", "").await;
    let unknown = fixture.text(HEALTHY, "no_such_method", "").await;

    let Err(CallError::Answered(failed)) = failed else {
        panic!("{failed:?}");
    };
    assert_eq!(failed.code, ErrorCode::Failed);
    assert_eq!(failed.message, "the method fails");
    let Err(CallError::Answered(unknown)) = unknown else {
        panic!("{unknown:?}");
    };
    assert_eq!(unknown.code, ErrorCode::NotFound);
    // An error is an answer: the process continues to run.
    assert_eq!(fixture.run_state(HEALTHY), RunState::Running);
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_call_that_the_caller_drops_is_cancelled_in_the_extension() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);
    let mut events = fixture.host.subscribe();
    let before = fixture.process_info(HEALTHY).await;

    let call = fixture.text(HEALTHY, "slow", "");
    let dropped = tokio::time::timeout(Duration::from_millis(200), call).await;

    assert!(dropped.is_err());
    cancellation_seen(&mut events).await;
    // The extension answered the cancelled call and takes the next one.
    assert_eq!(fixture.process_info(HEALTHY).await.pid, before.pid);
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_call_over_the_time_limit_fails_and_is_cancelled_in_the_extension() {
    let fixture = Fixture::with_limits(|limits| limits.call = Duration::from_millis(300));
    fixture.install(HEALTHY);
    let mut events = fixture.host.subscribe();

    let late = fixture.text(HEALTHY, "slow", "").await;

    assert_eq!(late, Err(CallError::Timeout));
    cancellation_seen(&mut events).await;
    assert_eq!(fixture.run_state(HEALTHY), RunState::Running);
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_call_that_is_larger_than_one_frame_is_refused_and_not_sent() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);
    let too_large = vec![0; 4 * 1024 * 1024];

    let refused = within(fixture.host.call(HEALTHY, "echo", too_large)).await;

    assert!(
        matches!(refused, Err(CallError::TooLarge { length }) if length > 4 * 1024 * 1024),
        "{refused:?}"
    );
    // The extension did not get a frame over the limit: it runs.
    assert_eq!(
        fixture.text(HEALTHY, "echo", "small").await.as_deref(),
        Ok("small")
    );
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn an_extension_calls_a_host_service_only_with_the_scope_that_the_user_approved() {
    let fixture = Fixture::new();
    let sessions_read = Permissions {
        scopes: vec![SESSIONS_READ.to_owned()],
        ..Permissions::default()
    };
    fixture.install_with("test.approved", sessions_read);
    // The manifest of this extension asks for the same scope, but the user
    // did not approve it.
    fixture.install("test.refused");

    let approved = fixture
        .text("test.approved", "ask_host", "sessions.list")
        .await;
    let refused = fixture
        .text("test.refused", "ask_host", "sessions.list")
        .await;
    let unknown = fixture
        .text("test.approved", "ask_host", "sessions.remove")
        .await;

    assert_eq!(approved.as_deref(), Ok("sessions for test.approved"));
    // The extension gives the error of the host back as its own answer.
    let Err(CallError::Answered(refused)) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(refused.code, ErrorCode::PermissionDenied);
    let Err(CallError::Answered(unknown)) = unknown else {
        panic!("{unknown:?}");
    };
    assert_eq!(unknown.code, ErrorCode::NotFound);
    // `Hello` tells each extension what the user approved.
    let scopes = |process: ProcessInfo| process.scopes;
    assert_eq!(
        scopes(fixture.process_info("test.approved").await),
        [SESSIONS_READ]
    );
    let no_scopes: [&str; 0] = [];
    assert_eq!(
        scopes(fixture.process_info("test.refused").await),
        no_scopes
    );
    fixture.host.shutdown().await;
}
