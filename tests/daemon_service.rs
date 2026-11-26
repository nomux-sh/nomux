//! `nomux daemon install-service` and `uninstall-service`: the file that
//! starts the daemon at login, in the configuration directory of the user.

use std::fs;
use std::path::PathBuf;

use nomux_end_to_end::{Sandbox, binary};

/// The service file of this operating system, and the link that the
/// service manager makes when the user enables the service.
fn service_paths(sandbox: &Sandbox) -> (PathBuf, Option<PathBuf>) {
    if cfg!(target_os = "macos") {
        let agents = sandbox.home().join("Library").join("LaunchAgents");
        (agents.join("sh.nomux.nomuxd.plist"), None)
    } else {
        let units = sandbox.home().join("config").join("systemd").join("user");
        let link = units.join("default.target.wants").join("nomuxd.service");
        (units.join("nomuxd.service"), Some(link))
    }
}

#[test]
fn the_service_file_is_written_and_removed_only_in_the_directory_of_the_user() {
    let mut sandbox = Sandbox::new();
    sandbox.write_configuration("");
    let (file, enabled_link) = service_paths(&sandbox);
    let daemon = binary("nomuxd");
    let daemon = daemon.to_str().unwrap();

    // A dry run shows the file and writes nothing.
    let shown = sandbox
        .nomux_ok(&["daemon", "install-service", "--dry-run"])
        .stdout;
    assert!(shown.contains(file.to_str().unwrap()), "{shown}");
    assert!(shown.contains(daemon), "{shown}");
    assert!(!file.exists());
    assert_eq!(
        sandbox.nomux_ok(&["daemon", "uninstall-service"]).stdout,
        format!("no service file: {} does not exist\n", file.display())
    );

    let installed = sandbox
        .nomux_ok(&["daemon", "install-service", "--json"])
        .json();
    assert_eq!(installed["service"]["action"], "installed");
    assert_eq!(installed["service"]["path"], file.to_str().unwrap());
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(installed["service"]["content"], content.as_str());
    assert!(content.contains(daemon), "{content}");
    if !cfg!(target_os = "macos") {
        // A stop of the unit must not stop the holders.
        assert!(content.contains("\nKillMode=process\n"), "{content}");
    }

    // The user enabled the service: the link has no target after the
    // removal of the file, thus the command removes it too.
    if let Some(link) = &enabled_link {
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&file, link).unwrap();
    }
    let dry_run = sandbox
        .nomux_ok(&["daemon", "uninstall-service", "--dry-run"])
        .stdout;
    assert_eq!(dry_run, format!("would remove {}\n", file.display()));
    assert!(file.exists());
    let removed = sandbox
        .nomux_ok(&["daemon", "uninstall-service", "--json"])
        .json();
    assert_eq!(removed["service"]["action"], "removed");
    assert!(!file.exists());
    if let Some(link) = &enabled_link {
        assert!(fs::symlink_metadata(link).is_err());
    }

    // The commands start no daemon, and they do not use a remote host.
    assert_eq!(sandbox.nomux(&["daemon", "status"]).code, 2);
    let remote = sandbox.nomux(&["--host", "vps", "daemon", "install-service"]);
    assert_eq!(remote.code, 1, "{remote:?}");
}
