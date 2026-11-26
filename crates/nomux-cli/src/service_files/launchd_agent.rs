//! The launchd agent of the daemon.

use std::path::{Path, PathBuf};

pub(super) const LABEL: &str = "sh.nomux.nomuxd";

/// The file of the agent in the directory of the agents of the user.
pub(super) fn path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{LABEL}.plist"))
}

/// The text of the property list for the daemon binary `daemon`.
///
/// - `RunAtLoad`: the daemon starts at login.
/// - `KeepAlive` with `SuccessfulExit` false: a daemon that fails starts
///   again. A daemon that a client stopped stays stopped.
/// - `AbandonProcessGroup`: when the agent stops, launchd does not stop the
///   other processes of the process group of the daemon. A holder has its
///   own process group, thus the key says the same rule a second time.
/// - `--replace`: the daemon of the agent takes the place of a daemon that
///   a client started before. No session ends.
pub(super) fn content(daemon: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
		<string>--replace</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>AbandonProcessGroup</key>
	<true/>
	<key>ProcessType</key>
	<string>Background</string>
</dict>
</plist>
"#,
        xml_text(&daemon.to_string_lossy())
    )
}

fn xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_of_the_daemon_is_text_of_the_property_list() {
        let agent = content(Path::new("/Users/a&b/<x>/nomuxd"));

        assert!(
            agent.contains("<string>/Users/a&amp;b/&lt;x&gt;/nomuxd</string>"),
            "{agent}"
        );
        assert_eq!(
            path(Path::new("/Users/user")),
            Path::new("/Users/user/Library/LaunchAgents/sh.nomux.nomuxd.plist")
        );
    }
}
