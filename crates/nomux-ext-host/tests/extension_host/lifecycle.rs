//! The life cycle of a process: start on the first call, stop when idle,
//! stop with the host.

use std::time::Duration;

use nomux_ext_host::RunState;

use crate::fixture::{Fixture, HEALTHY, alive_tasks};

#[tokio::test]
async fn the_first_call_starts_the_process_and_an_idle_process_stops() {
    let fixture = Fixture::with_limits(|limits| limits.idle = Duration::from_millis(300));
    fixture.install(HEALTHY);
    // An installed extension without a call costs nothing: no task, thus
    // no process and no timer.
    assert_eq!(fixture.run_state(HEALTHY), RunState::Stopped);
    assert_eq!(alive_tasks(), 0);

    let first = fixture.process_info(HEALTHY).await;
    assert_eq!(fixture.run_state(HEALTHY), RunState::Running);
    assert_eq!(alive_tasks(), 1);

    fixture
        .wait_for_state(HEALTHY, |state| *state == RunState::Stopped)
        .await;
    assert_eq!(alive_tasks(), 0);

    let second = fixture.process_info(HEALTHY).await;
    assert_ne!(first.pid, second.pid);
    assert_eq!(fixture.run_state(HEALTHY), RunState::Running);
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn calls_that_arrive_together_use_one_process() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);

    let (first, second) =
        tokio::join!(fixture.process_info(HEALTHY), fixture.process_info(HEALTHY));

    assert_eq!(first.pid, second.pid);
    assert_eq!(alive_tasks(), 1);
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn events_keep_a_process_only_while_a_subscriber_gets_them() {
    let fixture = Fixture::with_limits(|limits| limits.idle = Duration::from_millis(500));
    fixture.install(HEALTHY);
    let subscriber = fixture.host.subscribe();
    let first = fixture.process_info(HEALTHY).await;

    // The extension sends an event each 50 milliseconds from now on.
    fixture.text(HEALTHY, "start_ticking", "").await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(fixture.run_state(HEALTHY), RunState::Running);

    drop(subscriber);
    fixture
        .wait_for_state(HEALTHY, |state| *state == RunState::Stopped)
        .await;
    assert_ne!(fixture.process_info(HEALTHY).await.pid, first.pid);
    fixture.host.shutdown().await;
}

#[tokio::test]
async fn shutdown_stops_each_process_as_the_protocol_says() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);
    fixture.install("test.second");
    fixture.process_info(HEALTHY).await;
    fixture.process_info("test.second").await;
    assert_eq!(alive_tasks(), 2);

    fixture.host.shutdown().await;

    // `Stopped` and not `Failed`: each process answered the shutdown
    // request and ended with the status 0.
    assert_eq!(fixture.run_state(HEALTHY), RunState::Stopped);
    assert_eq!(fixture.run_state("test.second"), RunState::Stopped);
    assert_eq!(alive_tasks(), 0);
}

#[tokio::test]
async fn the_process_gets_its_own_directory_and_only_the_environment_of_the_host() {
    let fixture = Fixture::new();
    fixture.install(HEALTHY);

    let process = fixture.process_info(HEALTHY).await;

    let expected_directory = fixture.host_directory().join("data").join(HEALTHY);
    assert_eq!(
        std::fs::canonicalize(&process.working_directory).unwrap(),
        std::fs::canonicalize(expected_directory).unwrap()
    );
    // Cargo gives the test many variables, for example
    // `CARGO_MANIFEST_DIR`. The extension gets none of them.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    assert!(process.variables.contains(&"PATH".to_owned()));
    let not_from_the_host = |name: &&String| !["PATH", "HOME"].contains(&name.as_str());
    let leaked: Vec<&String> = process.variables.iter().filter(not_from_the_host).collect();
    assert!(leaked.is_empty(), "{leaked:?}");
    fixture.host.shutdown().await;
}
