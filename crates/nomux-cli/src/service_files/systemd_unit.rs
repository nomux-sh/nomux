//! The systemd user unit of the daemon.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(super) const UNIT: &str = "nomuxd.service";

/// The directory of the units of the user: `systemd/user` in
/// `XDG_CONFIG_HOME`, or in `~/.config`.
pub(super) fn directory(home: &Path, xdg_config_home: Option<OsString>) -> PathBuf {
    xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
        .join("systemd")
        .join("user")
}

/// The link that `systemctl --user enable` makes for the unit.
pub(super) fn enabled_link(directory: &Path) -> PathBuf {
    directory.join("default.target.wants").join(UNIT)
}

/// The text of the unit for the daemon binary `daemon`.
///
/// - `KillMode=process`: a stop of the unit stops only the daemon. With the
///   default mode, systemd stops each process of the control group of the
///   unit, and the holders are in that group: each session ends.
/// - `--replace`: the daemon of the unit takes the place of a daemon that a
///   client started before. No session ends.
/// - `Restart=on-failure`: a daemon that fails starts again. A daemon that
///   a client stopped has the exit status 0 and stays stopped.
pub(super) fn content(daemon: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=nomux daemon\n\
         Documentation=https://nomux-sh.github.io/docs/\n\
         \n\
         [Service]\n\
         ExecStart={} --replace\n\
         KillMode=process\n\
         Restart=on-failure\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        quoted(&daemon.to_string_lossy())
    )
}

/// One word of a command line of a unit file. In such a line, a space ends
/// a word, `%` starts a specifier and `$` starts a variable.
fn quoted(word: &str) -> String {
    let mut quoted = String::from("\"");
    for character in word.chars() {
        match character {
            '"' | '\\' => quoted.extend(['\\', character]),
            '%' | '$' => quoted.extend([character, character]),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stop_of_the_unit_stops_only_the_daemon() {
        let unit = content(Path::new("/opt/nomux/nomuxd"));

        assert!(unit.contains("\nKillMode=process\n"), "{unit}");
        assert!(
            unit.contains("\nExecStart=\"/opt/nomux/nomuxd\" --replace\n"),
            "{unit}"
        );
    }

    #[test]
    fn the_unit_links_to_the_documentation_site() {
        let unit = content(Path::new("/opt/nomux/nomuxd"));

        assert!(
            unit.contains("\nDocumentation=https://nomux-sh.github.io/docs/\n"),
            "{unit}"
        );
    }

    #[test]
    fn a_path_is_one_word_without_a_specifier_or_a_variable() {
        let unit = content(Path::new("/home/a b/50%/$HOME/\"x\"\\nomuxd"));

        assert!(
            unit.contains("ExecStart=\"/home/a b/50%%/$$HOME/\\\"x\\\"\\\\nomuxd\" --replace\n"),
            "{unit}"
        );
    }

    #[test]
    fn the_unit_is_in_the_configuration_directory_of_the_user() {
        let home = Path::new("/home/user");

        assert_eq!(
            directory(home, None),
            Path::new("/home/user/.config/systemd/user")
        );
        assert_eq!(
            directory(home, Some("/xdg".into())),
            Path::new("/xdg/systemd/user")
        );
        // A relative path is not a configuration directory.
        assert_eq!(
            directory(home, Some("relative".into())),
            Path::new("/home/user/.config/systemd/user")
        );
    }
}
