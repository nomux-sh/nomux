//! The adapter of a tool: an enabled extension whose manifest names the
//! tool in its `[adapter]` table and that has the approved scope
//! `hooks:read`.

use std::collections::BTreeMap;

use nomux_ext_host::{Enablement, ExtensionHost, ExtensionStatus};

/// The scope that an adapter needs to get the hook events of its tools.
const HOOKS_READ: &str = "hooks:read";

/// The method of an adapter that takes one hook event: a `HookEvent`.
pub(super) const HOOK_METHOD: &str = "hook";
/// The method of an adapter that describes the change that its hooks need
/// in the settings file of the tool: a `HookInstallPlan`.
pub(crate) const PLAN_METHOD: &str = "hooks.plan";

/// Each tool that has an adapter, with the identifier of its adapter. If
/// more than one extension names a tool, the extension with the smallest
/// identifier is its adapter.
pub(crate) fn adapters(host: &ExtensionHost) -> BTreeMap<String, String> {
    let mut adapters = BTreeMap::new();
    // The host gives the extensions in the sequence of their identifiers.
    for status in host
        .extensions()
        .iter()
        .filter(|status| takes_hooks(status))
    {
        let extension = &status.extension;
        let tools = extension
            .manifest
            .adapter
            .iter()
            .flat_map(|adapter| &adapter.tools);
        for tool in tools {
            adapters
                .entry(tool.clone())
                .or_insert_with(|| extension.manifest.id.clone());
        }
    }
    adapters
}

/// The identifier of the adapter of `tool`. `None` if the tool has no
/// adapter.
pub(crate) fn adapter_of(host: &ExtensionHost, tool: &str) -> Option<String> {
    adapters(host).remove(tool)
}

fn takes_hooks(status: &ExtensionStatus) -> bool {
    let extension = &status.extension;
    extension.enablement == Enablement::Enabled
        && extension
            .granted
            .scopes
            .iter()
            .any(|scope| scope == HOOKS_READ)
}
