//! A host in a temporary directory, and packages of the test extension.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nomux_ext_host::{
    CallError, ExtensionHost, HostConfig, HostServices, Limits, Permissions, RunState,
    ServiceError, ServiceReply,
};
use prost::Message;
use tempfile::TempDir;

use crate::messages::{ProcessInfo, Text};

const TEST_EXTENSION: &str = env!("CARGO_BIN_EXE_nomux-test-extension");

/// An extension that does not fail. The tests use it to show that the
/// failure of a different extension does not affect the host.
pub(crate) const HEALTHY: &str = "test.healthy";

/// The time after which a test fails when the host does not answer.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The scope that the manifest of each test package asks for.
pub(crate) const SESSIONS_READ: &str = "sessions:read";

/// The host services of the tests: one method that needs a scope.
struct Sessions;

impl HostServices for Sessions {
    fn required_scope(&self, method: &str) -> Option<String> {
        (method == "sessions.list").then(|| SESSIONS_READ.to_owned())
    }

    fn call(&self, extension_id: &str, _method: &str, _payload: Vec<u8>) -> ServiceReply {
        let text = format!("sessions for {extension_id}");
        Box::pin(async move { Ok::<_, ServiceError>(Text { text }.encode_to_vec()) })
    }
}

pub(crate) struct Fixture {
    directory: TempDir,
    limits: Limits,
    pub(crate) host: ExtensionHost,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        Self::with_limits(|_| {})
    }

    /// A host with the default limits, a short pause before a new start,
    /// and the changes of the test.
    pub(crate) fn with_limits(change: impl FnOnce(&mut Limits)) -> Self {
        let mut limits = Limits::default();
        short_restart_pause(&mut limits);
        change(&mut limits);
        let directory = tempfile::tempdir().expect("the test helper works");
        let host = open(&directory.path().join("host"), &limits);
        Self {
            directory,
            limits,
            host,
        }
    }

    pub(crate) fn host_directory(&self) -> PathBuf {
        self.directory.path().join("host")
    }

    /// A second host on the same directory, as after a restart of the
    /// daemon.
    pub(crate) fn reopen(&self) -> ExtensionHost {
        open(&self.host_directory(), &self.limits)
    }

    /// Makes a package of the test extension. Its manifest is valid and
    /// asks for one scope.
    pub(crate) fn package(&self, id: &str) -> PathBuf {
        let package = self.directory.path().join("sources").join(id);
        fs::create_dir_all(package.join("bin")).expect("the test helper works");
        fs::write(package.join("extension.toml"), manifest(id, "^0.1"))
            .expect("the test helper works");
        fs::copy(TEST_EXTENSION, package.join("bin").join(id)).expect("the test helper works");
        package
    }

    /// Installs a package of the test extension without permissions.
    pub(crate) fn install(&self, id: &str) {
        self.install_with(id, Permissions::default());
    }

    pub(crate) fn install_with(&self, id: &str, granted: Permissions) {
        self.host
            .install(&self.package(id), granted)
            .expect("the test helper works");
    }

    /// Calls a method whose argument and result are a text.
    pub(crate) async fn text(
        &self,
        id: &str,
        method: &str,
        text: &str,
    ) -> Result<String, CallError> {
        text_call(&self.host, id, method, text).await
    }

    pub(crate) async fn process_info(&self, id: &str) -> ProcessInfo {
        let payload = within(self.host.call(id, "process", Vec::new())).await;
        ProcessInfo::decode(payload.expect("the test helper works").as_slice())
            .expect("the test helper works")
    }

    pub(crate) fn run_state(&self, id: &str) -> RunState {
        self.host
            .extension(id)
            .expect("the test helper works")
            .run_state
    }

    /// Waits until the process of an extension has a state that the test
    /// accepts.
    pub(crate) async fn wait_for_state(
        &self,
        id: &str,
        accept: impl FnMut(&RunState) -> bool,
    ) -> RunState {
        let mut states = self
            .host
            .watch_run_state(id)
            .expect("the test helper works");
        let state = within(states.wait_for(accept))
            .await
            .expect("the test helper works");
        state.clone()
    }

    /// Waits until the pause after a failure of an extension has ended.
    pub(crate) async fn wait_for_restart_pause(&self, id: &str) {
        let RunState::Failed {
            retry_at: Some(retry_at),
            ..
        } = self.run_state(id)
        else {
            panic!("the extension has no pause: {:?}", self.run_state(id));
        };
        tokio::time::sleep(retry_at.saturating_duration_since(Instant::now())).await;
    }
}

fn short_restart_pause(limits: &mut Limits) {
    limits.restart_delay_first = Duration::from_millis(20);
}

fn open(directory: &Path, limits: &Limits) -> ExtensionHost {
    let mut config = HostConfig::new(directory);
    config.limits = limits.clone();
    ExtensionHost::open(config, Arc::new(Sessions)).expect("the test helper works")
}

pub(crate) fn manifest(id: &str, api: &str) -> String {
    format!(
        r#"
[extension]
id = "{id}"
name = "Test"
version = "1.0.0"
api = "{api}"

[daemon]
command = "bin/{id}"

[permissions]
scopes = ["{SESSIONS_READ}"]
"#
    )
}

pub(crate) async fn text_call(
    host: &ExtensionHost,
    id: &str,
    method: &str,
    text: &str,
) -> Result<String, CallError> {
    let input = Text {
        text: text.to_owned(),
    };
    let payload = within(host.call(id, method, input.encode_to_vec())).await?;
    Ok(Text::decode(payload.as_slice())
        .expect("the test helper works")
        .text)
}

/// Fails the test if the future does not complete in the time of a test.
pub(crate) async fn within<Output>(future: impl Future<Output = Output>) -> Output {
    tokio::time::timeout(TIMEOUT, future)
        .await
        .expect("the host answers in the time of a test")
}

/// The number of tasks of the runtime of the test, without the test.
pub(crate) fn alive_tasks() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}
