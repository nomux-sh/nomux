//! The hooks of the agent tools: a hook event, the adapters and the install
//! plan of an adapter.

use nomux_proto::wire::v1 as wire;

/// One event of a hook of an agent tool in a session.
#[derive(Clone, PartialEq, Eq)]
pub struct HookEvent {
    /// The identifier of the session: the value of `NOMUX_SESSION_ID`.
    pub session_id: String,
    /// The token of the session: the value of `NOMUX_TOKEN`.
    pub token: String,
    /// The name of the tool.
    pub tool: String,
    /// The name of the event.
    pub event: String,
    /// The standard input of the hook. Empty if it was too large.
    pub payload: Vec<u8>,
    /// `true` if the standard input of the hook was too large to send.
    pub payload_omitted: bool,
}

/// The token is a secret: the debug output does not have it.
impl std::fmt::Debug for HookEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookEvent")
            .field("session_id", &self.session_id)
            .field("tool", &self.tool)
            .field("event", &self.event)
            .field("payload_length", &self.payload.len())
            .field("payload_omitted", &self.payload_omitted)
            .finish_non_exhaustive()
    }
}

/// A tool and the extension that is its adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookAdapter {
    /// The name of the tool.
    pub tool: String,
    /// The identifier of the adapter extension.
    pub extension_id: String,
}

/// An operating system of a settings path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPlatform {
    /// Linux.
    Linux,
    /// macOS.
    Macos,
    /// A platform that this client does not know.
    Unknown,
}

/// The path of the settings file of a tool on one platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPath {
    /// The platform.
    pub platform: HookPlatform,
    /// The path below the home directory: it starts with `~/`. The adapter
    /// gives it: check it before use.
    pub path: String,
}

/// One entry that the hooks need in a JSON settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsEntry {
    /// A JSON pointer to an array.
    pub pointer: String,
    /// The JSON value that the array must contain, as JSON text.
    pub value_json: String,
}

/// The change that the hooks of a tool need in its settings file, as its
/// adapter describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInstallPlan {
    /// The identifier of the adapter that gave the plan.
    pub extension_id: String,
    /// The settings file on each platform.
    pub settings_paths: Vec<SettingsPath>,
    /// The entries, in the sequence in which they are added.
    pub entries: Vec<SettingsEntry>,
}

impl From<wire::HookAdapter> for HookAdapter {
    fn from(adapter: wire::HookAdapter) -> Self {
        Self {
            tool: adapter.tool,
            extension_id: adapter.extension_id,
        }
    }
}

impl HookInstallPlan {
    pub(crate) fn new(extension_id: String, plan: wire::HookInstallPlan) -> Self {
        let platform = |value| match wire::HookPlatform::try_from(value) {
            Ok(wire::HookPlatform::Linux) => HookPlatform::Linux,
            Ok(wire::HookPlatform::Macos) => HookPlatform::Macos,
            Ok(wire::HookPlatform::Unspecified) | Err(_) => HookPlatform::Unknown,
        };
        Self {
            extension_id,
            settings_paths: plan
                .settings_paths
                .into_iter()
                .map(|path| SettingsPath {
                    platform: platform(path.platform),
                    path: path.path,
                })
                .collect(),
            entries: plan
                .entries
                .into_iter()
                .map(|entry| SettingsEntry {
                    pointer: entry.pointer,
                    value_json: entry.value_json,
                })
                .collect(),
        }
    }
}
