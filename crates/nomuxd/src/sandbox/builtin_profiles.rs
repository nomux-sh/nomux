//! The built-in sandbox profiles and their data: the system paths that a
//! command reads, the secret paths of the home directory that it never
//! reads, and the state of the agent tools. `docs/sandbox.md` has the same
//! lists: change the two together.

use nomux_holder_protocol::sandbox::{FeatureSet, SandboxFeature};

/// The profile that is no sandbox.
pub(super) const OFF: &str = "off";

/// What a built-in profile permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BuiltinProfile {
    pub(super) name: &'static str,
    /// The command can change the project directory. Else it can read it.
    pub(super) writes_project: bool,
    pub(super) network_off: bool,
}

pub(super) const BUILTIN_PROFILES: [BuiltinProfile; 3] = [
    BuiltinProfile {
        name: "project",
        writes_project: true,
        network_off: false,
    },
    BuiltinProfile {
        name: "project-offline",
        writes_project: true,
        network_off: true,
    },
    BuiltinProfile {
        name: "read-only",
        writes_project: false,
        network_off: true,
    },
];

/// The features that each profile wants and does not require: a kernel
/// without them starts the session in the partial state.
pub(super) fn wanted_features() -> FeatureSet {
    [
        SandboxFeature::UnixSockets,
        SandboxFeature::Signals,
        SandboxFeature::AbstractSockets,
    ]
    .into_iter()
    .collect()
}

/// The features that a profile requires.
pub(super) fn required_features(network_off: bool) -> FeatureSet {
    let files = FeatureSet::EMPTY.with(SandboxFeature::Files);
    if network_off {
        files.with(SandboxFeature::NetworkOff)
    } else {
        files
    }
}

/// The system paths that a command reads and runs: the programs, their
/// libraries and their configuration. A path that does not exist adds no
/// rule.
pub(super) const SYSTEM_READ: [&str; 12] = [
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/opt",
    "/etc",
    "/sys",
    "/proc",
    "/var/lib",
    "/run/systemd/resolve",
];

/// The system paths that a command changes: the terminals, `/dev/null` and
/// the shared memory of `/dev/shm`.
pub(super) const SYSTEM_WRITE: [&str; 1] = ["/dev"];

/// The paths below the home directory that a sandboxed command never reads:
/// keys, tokens with power over infrastructure, publish tokens, browser
/// data, the logins of other agent tools, the sockets and journals of
/// nomux, and the history of the shells. `~/.nomux/bin` stays readable: a
/// process of the session runs `nomux` from there for `nomux status set`.
pub(super) const SECRET_PATHS: [&str; 53] = [
    ".ssh",
    ".gnupg",
    ".password-store",
    ".local/share/keyrings",
    ".config/1Password",
    ".config/Bitwarden",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".kube",
    ".docker",
    ".config/gh",
    ".config/glab-cli",
    ".netrc",
    ".git-credentials",
    ".terraform.d",
    ".config/doctl",
    ".cloudflared",
    ".config/helm",
    ".npmrc",
    ".pypirc",
    ".cargo/credentials.toml",
    ".cargo/credentials",
    ".gem/credentials",
    ".m2/settings.xml",
    ".mozilla",
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/microsoft-edge",
    ".var",
    ".claude",
    ".claude.json",
    ".codex",
    ".gemini",
    ".config/opencode",
    ".cursor",
    ".copilot",
    ".config/github-copilot",
    ".grok",
    ".nomux/run",
    ".nomux/downloads",
    ".local/state/nomux",
    ".local/share/nomux",
    ".config/nomux",
    ".bash_history",
    ".zsh_history",
    ".local/share/fish/fish_history",
    ".python_history",
    ".node_repl_history",
    ".psql_history",
    ".mysql_history",
    ".lesshst",
];

/// The state that an agent tool changes in the home directory, by the name
/// of its program. The paths leave the secret list for a session of the
/// tool, and the command can change them.
///
/// The adapter of a tool declares this in a later version: the manifest of
/// an extension has no field for it yet.
pub(super) const TOOL_STATE: [(&str, &[&str]); 1] = [("claude", &[".claude", ".claude.json"])];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_state_is_in_the_secret_list_and_no_path_is_twice() {
        for (_, paths) in TOOL_STATE {
            for path in paths {
                assert!(SECRET_PATHS.contains(path), "{path}");
            }
        }
        let mut sorted = SECRET_PATHS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), SECRET_PATHS.len());
    }
}
