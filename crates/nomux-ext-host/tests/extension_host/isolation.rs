//! Failures of an extension: each one stays in its process. The host stops
//! the process, starts the extension again for a later call, and continues
//! to serve a second extension.

use std::time::Duration;

use nomux_ext_host::{CallError, Enablement, Failure, Limits, RunState};

use crate::fixture::{Fixture, HEALTHY, text_call};

const FAULTY: &str = "test.faulty";

/// A host with an extension that the test makes fail and one that does not
/// fail.
fn fixture_with_limits(change: impl FnOnce(&mut Limits)) -> Fixture {
    let fixture = Fixture::with_limits(change);
    fixture.install(FAULTY);
    fixture.install(HEALTHY);
    fixture
}

/// Calls a method that makes the process of the faulty extension fail, and
/// returns the failure that the caller gets.
async fn failure_of(fixture: &Fixture, method: &str) -> Failure {
    let result = fixture.text(FAULTY, method, "").await;
    let Err(CallError::Failed(failure)) = result else {
        panic!("`{method}` did not fail the process: {result:?}");
    };
    failure
}

/// After a failure: the host marks the extension, serves the healthy
/// extension, and starts the faulty extension again after the pause.
async fn assert_contained(fixture: &Fixture, failure: &Failure) {
    assert!(
        matches!(fixture.run_state(FAULTY), RunState::Failed { failure: marked, .. } if marked == *failure),
        "{:?}",
        fixture.run_state(FAULTY)
    );
    let neighbour = fixture.text(HEALTHY, "echo", "still here").await;
    assert_eq!(neighbour.as_deref(), Ok("still here"));

    fixture.wait_for_restart_pause(FAULTY).await;
    let again = fixture.text(FAULTY, "echo", "again").await;
    assert_eq!(again.as_deref(), Ok("again"));
    assert_eq!(fixture.run_state(FAULTY), RunState::Running);
}

fn exit_code(failure: &Failure) -> Option<i32> {
    match failure {
        Failure::Exited(status) => status.code(),
        _ => None,
    }
}

#[tokio::test]
async fn a_process_that_ends_in_a_call_fails_the_call_and_starts_again_later() {
    let fixture = fixture_with_limits(|_| {});
    fixture.text(FAULTY, "echo", "start").await.unwrap();

    let failure = failure_of(&fixture, "crash").await;

    assert_eq!(exit_code(&failure), Some(7), "{failure:?}");
    assert_contained(&fixture, &failure).await;
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_process_that_does_not_answer_health_requests_is_ended() {
    let fixture = fixture_with_limits(|limits| {
        limits.health_interval = Duration::from_millis(100);
        limits.health = Duration::from_millis(500);
    });

    let failure = failure_of(&fixture, "hang").await;

    assert_eq!(failure, Failure::HealthTimeout);
    assert_contained(&fixture, &failure).await;
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_frame_that_is_not_valid_or_too_large_ends_the_process() {
    let fixture = fixture_with_limits(|_| {});

    let bad = failure_of(&fixture, "bad_frame").await;
    assert!(matches!(bad, Failure::BadFrame(_)), "{bad:?}");
    assert_contained(&fixture, &bad).await;

    let oversized = failure_of(&fixture, "oversized_frame").await;
    let over_the_limit = 4 * 1024 * 1024 + 1;
    assert_eq!(
        oversized,
        Failure::OversizedFrame {
            length: over_the_limit
        }
    );
    assert_contained(&fixture, &oversized).await;
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_flood_on_the_standard_error_ends_the_process_and_the_log_stays_small() {
    let fixture = fixture_with_limits(|limits| limits.stderr_log_bytes = 1024);

    let failure = failure_of(&fixture, "flood_stderr").await;

    assert_eq!(failure, Failure::StderrFlood);
    assert_eq!(fixture.host.stderr_log(FAULTY), vec![b'x'; 1024]);
    assert_contained(&fixture, &failure).await;
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_call_that_the_extension_does_not_stop_after_a_cancellation_ends_the_process() {
    let fixture = fixture_with_limits(|limits| {
        limits.call = Duration::from_millis(200);
        limits.cancel = Duration::from_millis(300);
    });

    let late = fixture.text(FAULTY, "ignore_cancel", "").await;

    assert_eq!(late, Err(CallError::Timeout));
    let is_failed = |state: &RunState| matches!(state, RunState::Failed { .. });
    let state = fixture.wait_for_state(FAULTY, is_failed).await;
    assert!(
        matches!(
            &state,
            RunState::Failed {
                failure: Failure::CancelTimeout,
                ..
            }
        ),
        "{state:?}"
    );
    assert_contained(&fixture, &Failure::CancelTimeout).await;
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_process_that_ends_at_its_start_fails_the_call_and_its_last_words_are_in_the_log() {
    let fixture = Fixture::new();
    fixture.install("test.crash-at-start");
    fixture.install(HEALTHY);

    let result = fixture.text("test.crash-at-start", "echo", "").await;

    let Err(CallError::Failed(failure)) = result else {
        panic!("{result:?}");
    };
    assert_eq!(exit_code(&failure), Some(3), "{failure:?}");
    let log = String::from_utf8(fixture.host.stderr_log("test.crash-at-start")).unwrap();
    assert_eq!(log, "the test extension cannot start\n");
    let neighbour = fixture.text(HEALTHY, "echo", "still here").await;
    assert_eq!(neighbour.as_deref(), Ok("still here"));
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn a_process_that_does_not_complete_the_handshake_in_time_is_ended() {
    let fixture = Fixture::with_limits(|limits| limits.start = Duration::from_millis(300));
    fixture.install("test.slow-start");

    let result = fixture.text("test.slow-start", "echo", "").await;

    assert_eq!(result, Err(CallError::Failed(Failure::StartTimeout)));
}

#[tokio::test]
async fn a_call_in_the_pause_after_a_failure_does_not_start_the_process() {
    let fixture = fixture_with_limits(|limits| {
        limits.restart_delay_first = Duration::from_secs(60);
    });
    let failure = failure_of(&fixture, "crash").await;

    let in_the_pause = fixture.text(FAULTY, "echo", "").await;

    let Err(CallError::BackingOff {
        failure: last,
        retry_in,
    }) = in_the_pause
    else {
        panic!("{in_the_pause:?}");
    };
    assert_eq!(last, failure);
    assert!(retry_in > Duration::from_secs(50), "{retry_in:?}");
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn an_extension_that_fails_each_time_is_disabled_with_the_reason() {
    let fixture = Fixture::with_limits(|limits| limits.failures_before_disable = 3);
    fixture.install("test.crash-at-start");

    for _ in 0..2 {
        let result = fixture.text("test.crash-at-start", "echo", "").await;
        assert!(matches!(result, Err(CallError::Failed(_))), "{result:?}");
        fixture.wait_for_restart_pause("test.crash-at-start").await;
    }
    let third = fixture.text("test.crash-at-start", "echo", "").await;
    assert!(matches!(third, Err(CallError::Failed(_))), "{third:?}");

    let expected_reason = "the extension failed: the process ended: exit status: 3";
    let is_disabled = |result: Result<String, CallError>| matches!(&result, Err(CallError::Disabled { reason }) if reason == expected_reason);
    assert!(is_disabled(
        fixture.text("test.crash-at-start", "echo", "").await
    ));
    // The state is in the state file: a new daemon does not try again.
    let host = fixture.reopen();
    assert!(is_disabled(
        text_call(&host, "test.crash-at-start", "echo", "").await
    ));

    // The user can enable the extension again. The count starts again.
    fixture.host.enable("test.crash-at-start").await.unwrap();
    let after_enable = fixture.text("test.crash-at-start", "echo", "").await;
    assert!(
        matches!(after_enable, Err(CallError::Failed(_))),
        "{after_enable:?}"
    );
    let status = fixture.host.extension("test.crash-at-start").unwrap();
    assert_eq!(status.extension.enablement, Enablement::Enabled);
}

#[tokio::test]
async fn a_program_that_cannot_be_the_extension_of_the_manifest_is_disabled_immediately() {
    let fixture = Fixture::new();
    fixture.install("test.refuse-version");
    fixture.install("test.wrong-identity");

    let refused = fixture.text("test.refuse-version", "echo", "").await;
    let wrong = fixture.text("test.wrong-identity", "echo", "").await;

    assert!(
        matches!(
            refused,
            Err(CallError::Failed(Failure::VersionIncompatible(_)))
        ),
        "{refused:?}"
    );
    let claimed = "test.other".to_owned();
    assert_eq!(
        wrong,
        Err(CallError::Failed(Failure::IdentityMismatch { claimed }))
    );
    for id in ["test.refuse-version", "test.wrong-identity"] {
        let status = fixture.host.extension(id).unwrap();
        assert!(
            matches!(status.extension.enablement, Enablement::Disabled { .. }),
            "{status:?}"
        );
    }
}
