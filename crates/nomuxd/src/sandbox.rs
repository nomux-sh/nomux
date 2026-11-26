//! The sandbox of the sessions: which profile a session gets, the plan of
//! each start of its holder, and what the host can apply.
//! `docs/sandbox.md` has the rules.
//!
//! - The daemon resolves the profile into a plan of paths at each start of
//!   a holder: a new session, a restore and a wake. It writes the plan to a
//!   file of mode 0600 in the runtime directory, and the holder applies it
//!   to the command before the command starts.
//! - A sandboxed session never takes a pooled holder: a pooled shell runs
//!   before its session exists, and a sandbox cannot limit a process that
//!   runs.
//! - A holder that is too old for a sandbox never gets a plan: the session
//!   does not start.

mod agent_listener;
mod builtin_profiles;
mod home_listing;
mod peer_guard;
mod profile_resolver;
mod project_layout;

pub(crate) use self::agent_listener::serve_agent_socket;
pub(crate) use self::peer_guard::refuses;

use std::fs::{self, DirBuilder};
use std::io;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use nomux_core::configuration::SandboxSettings;
use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::AGENT_SOCKET_DIRECTORY;
use nomux_holder_protocol::sandbox::{FeatureSet, plan_file};
use nomux_sandbox::HostSupport;

use self::builtin_profiles::OFF;
use self::profile_resolver::{PlanInput, resolve};

const OWNER_ONLY: u32 = 0o700;
const OWNER_READ_WRITE: u32 = 0o600;

/// The sandbox settings of the daemon and what the host can apply.
pub(crate) struct SandboxPolicy {
    settings: SandboxSettings,
    support: HostSupport,
    runtime_directory: PathBuf,
    home: PathBuf,
}

/// The plan file of one start of a holder. The holder removes it; the
/// daemon removes it too after the start, also when the holder did not
/// start.
pub(crate) struct PlanFile {
    pub(crate) path: PathBuf,
}

impl Drop for PlanFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl SandboxPolicy {
    pub(crate) fn new(
        settings: SandboxSettings,
        runtime_directory: PathBuf,
        home: PathBuf,
    ) -> Self {
        Self {
            settings,
            support: nomux_sandbox::probe(),
            runtime_directory,
            home,
        }
    }

    /// What the host can apply.
    pub(crate) fn support(&self) -> &HostSupport {
        &self.support
    }

    /// The profile of a new session: the one of the request, else the
    /// default of the project of the working directory, else the default of
    /// the agent tool that is the command, else the default of the daemon.
    /// `None` for no sandbox.
    pub(crate) fn profile_of_new_session(
        &self,
        requested: Option<&str>,
        working_directory: &Path,
        program: &Path,
    ) -> Option<String> {
        let tool = program.file_name().and_then(|name| name.to_str());
        let project = self
            .settings
            .projects
            .iter()
            .filter(|(directory, _)| working_directory.starts_with(directory))
            .max_by_key(|(directory, _)| directory.as_os_str().len())
            .map(|(_, profile)| profile.as_str());
        requested
            .or(project)
            .or_else(|| tool.and_then(|tool| self.settings.tools.get(tool).map(String::as_str)))
            .or(self.settings.default.as_deref())
            .filter(|profile| *profile != OFF)
            .map(str::to_owned)
    }

    /// Resolves `profile` for one start of the holder of `id` and writes
    /// the plan file.
    ///
    /// # Errors
    ///
    /// Returns the reason, for a person, if the profile does not exist, if
    /// the host cannot apply a feature that it requires, or if a file
    /// cannot be written.
    pub(crate) fn write_plan(
        &self,
        id: &SessionId,
        profile: &str,
        working_directory: &Path,
        program: &Path,
        temporary: &Path,
    ) -> Result<PlanFile, String> {
        let agent_socket_directory = self.runtime_directory.join(AGENT_SOCKET_DIRECTORY);
        let executable = std::env::current_exe().ok();
        let input = PlanInput {
            profile,
            working_directory,
            program,
            home: &self.home,
            temporary,
            agent_socket_directory: &agent_socket_directory,
            binary_directory: executable.as_deref().and_then(Path::parent),
            settings: &self.settings,
        };
        let plan = resolve(&input)?.ok_or_else(|| "the profile off is no sandbox".to_owned())?;
        let missing = plan.require.without(self.support.features());
        if !missing.is_empty() {
            return Err(format!(
                "this host cannot apply {} of the sandbox profile {profile} ({}). The session \
                 does not start without them",
                missing.names().join(", "),
                self.support.detail()
            ));
        }
        DirBuilder::new()
            .recursive(true)
            .mode(OWNER_ONLY)
            .create(temporary)
            .map_err(|error| format!("cannot make {}: {error}", temporary.display()))?;
        let path = plan_file(&self.runtime_directory, id);
        let file = PlanFile { path };
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(OWNER_READ_WRITE)
            .open(&file.path)
            .and_then(|mut opened| opened.write_all(plan.encode().as_bytes()))
            .map_err(|error| format!("cannot write the sandbox plan: {error}"))?;
        Ok(file)
    }

    /// The features that a session with a profile wants and that the host
    /// cannot apply: the gaps of the partial state.
    pub(crate) fn missing_wanted(enforced: FeatureSet) -> FeatureSet {
        builtin_profiles::wanted_features().without(enforced)
    }
}

/// Makes the directory of the agent socket.
pub(crate) fn prepare_agent_socket_directory(runtime_directory: &Path) -> io::Result<PathBuf> {
    let directory = runtime_directory.join(AGENT_SOCKET_DIRECTORY);
    DirBuilder::new()
        .mode(OWNER_ONLY)
        .create(&directory)
        .or_else(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(error)
            }
        })?;
    Ok(directory)
}
