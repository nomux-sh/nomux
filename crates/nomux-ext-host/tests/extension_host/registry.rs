//! The installed extensions: installation, validation and the state that
//! stays after a restart of the daemon.

use std::fs;

use nomux_ext_host::{
    CallError, Enablement, ExtensionHost, InstallError, Permissions, RunState, StateError,
};

use crate::fixture::{Fixture, SESSIONS_READ, manifest, text_call};

fn sessions_read() -> Permissions {
    Permissions {
        scopes: vec![SESSIONS_READ.to_owned()],
        ..Permissions::default()
    }
}

#[tokio::test]
async fn an_installed_extension_is_a_copy_that_a_new_host_finds_with_its_permissions() {
    let fixture = Fixture::new();
    let source = fixture.package("test.git");

    let installed = fixture.host.install(&source, sessions_read()).unwrap();
    fs::remove_dir_all(source).unwrap();

    assert_eq!(installed.manifest.id, "test.git");
    let host = fixture.reopen();
    let found = host.extension("test.git").unwrap();
    assert_eq!(found.extension, installed);
    assert_eq!(found.extension.granted, sessions_read());
    assert_eq!(found.extension.enablement, Enablement::Enabled);
    assert_eq!(found.run_state, RunState::Stopped);
    let answer = text_call(&host, "test.git", "echo", "from the copy").await;
    assert_eq!(answer.as_deref(), Ok("from the copy"));
    host.shutdown().await;
}

#[tokio::test]
async fn a_package_that_is_not_valid_is_refused_and_not_installed() {
    let fixture = Fixture::new();
    let install = |id: &str, change: &dyn Fn(&std::path::Path), granted: Permissions| {
        let package = fixture.package(id);
        change(&package);
        fixture.host.install(&package, granted).unwrap_err()
    };
    let none = Permissions::default;

    let bad_manifest = install(
        "test.bad-manifest",
        &|package| fs::write(package.join("extension.toml"), "[extension]\nid = \"Bad\"").unwrap(),
        none(),
    );
    assert!(
        matches!(bad_manifest, InstallError::Manifest(_)),
        "{bad_manifest:?}"
    );

    let other_api = install(
        "test.other-api",
        &|package| {
            let text = manifest("test.other-api", "^9");
            fs::write(package.join("extension.toml"), text).unwrap();
        },
        none(),
    );
    assert!(
        matches!(other_api, InstallError::IncompatibleApi { .. }),
        "{other_api:?}"
    );

    let no_program = install(
        "test.no-program",
        &|package| fs::remove_file(package.join("bin/test.no-program")).unwrap(),
        none(),
    );
    assert!(
        matches!(&no_program, InstallError::MissingProgram { command } if command == "bin/test.no-program"),
        "{no_program:?}"
    );

    let link = install(
        "test.link",
        &|package| std::os::unix::fs::symlink("/etc", package.join("outside")).unwrap(),
        none(),
    );
    assert!(
        matches!(link, InstallError::UnsupportedFile { .. }),
        "{link:?}"
    );

    let network = Permissions {
        network: true,
        ..Permissions::default()
    };
    let not_requested = install("test.greedy", &|_| {}, network);
    assert!(
        matches!(&not_requested, InstallError::NotRequested { permission } if permission == "network"),
        "{not_requested:?}"
    );

    assert_eq!(fixture.host.extensions(), Vec::new());
    assert_eq!(fixture.reopen().extensions(), Vec::new());
}

#[tokio::test]
async fn an_extension_is_installed_one_time() {
    let fixture = Fixture::new();
    fixture.install("test.git");

    let again = fixture
        .host
        .install(&fixture.package("test.git"), Permissions::default());

    assert!(
        matches!(&again, Err(InstallError::AlreadyInstalled { id }) if id == "test.git"),
        "{again:?}"
    );
}

#[tokio::test]
async fn a_disabled_extension_is_stopped_and_refuses_calls_also_after_a_restart() {
    let fixture = Fixture::new();
    fixture.install("test.git");
    fixture.text("test.git", "echo", "start").await.unwrap();
    assert_eq!(fixture.run_state("test.git"), RunState::Running);

    fixture
        .host
        .disable("test.git", "the user disabled it")
        .await
        .unwrap();

    assert_eq!(fixture.run_state("test.git"), RunState::Stopped);
    let disabled = CallError::Disabled {
        reason: "the user disabled it".to_owned(),
    };
    let refused = fixture.text("test.git", "echo", "no").await;
    assert_eq!(refused, Err(disabled.clone()));
    let host = fixture.reopen();
    let refused_after_restart = text_call(&host, "test.git", "echo", "no").await;
    assert_eq!(refused_after_restart, Err(disabled));

    host.enable("test.git").await.unwrap();

    let answer = text_call(&host, "test.git", "echo", "yes").await;
    assert_eq!(answer.as_deref(), Ok("yes"));
    host.shutdown().await;
}

#[tokio::test]
async fn an_extension_for_a_different_api_version_is_listed_but_not_started() {
    let fixture = Fixture::new();
    fixture.install("test.git");
    // The host that installed the package supplied a different version of
    // the extension API than the host that runs now.
    let installed_manifest = fixture
        .host_directory()
        .join("packages/test.git/extension.toml");
    fs::write(installed_manifest, manifest("test.git", "^9")).unwrap();
    let host = fixture.reopen();

    let refused = text_call(&host, "test.git", "echo", "no").await;

    assert!(
        matches!(refused, Err(CallError::IncompatibleApi { .. })),
        "{refused:?}"
    );
    assert_eq!(
        host.extension("test.git").unwrap().run_state,
        RunState::Stopped
    );
}

#[tokio::test]
async fn an_extension_that_is_not_installed_cannot_be_called() {
    let fixture = Fixture::new();

    let refused = fixture.text("test.unknown", "echo", "no").await;

    assert_eq!(refused, Err(CallError::NotInstalled));
}

#[tokio::test]
async fn an_inspection_gives_the_request_of_the_manifest_and_installs_nothing() {
    let fixture = Fixture::new();
    let package = fixture.package("test.git");

    let manifest = ExtensionHost::inspect(&package).unwrap();

    assert_eq!(manifest.id, "test.git");
    assert_eq!(manifest.permissions.scopes, [SESSIONS_READ]);
    assert_eq!(fixture.host.extensions(), Vec::new());
    assert!(!fixture.host_directory().exists());

    // An inspection refuses what an installation refuses.
    fs::remove_file(package.join("bin/test.git")).unwrap();
    let no_program = ExtensionHost::inspect(&package);
    assert!(
        matches!(no_program, Err(InstallError::MissingProgram { .. })),
        "{no_program:?}"
    );
}

#[tokio::test]
async fn a_removed_extension_stops_and_leaves_no_file() {
    let fixture = Fixture::new();
    fixture.install_with("test.git", sessions_read());
    let process = fixture.process_info("test.git").await;
    let files = ["packages/test.git", "state/test.git.toml", "data/test.git"]
        .map(|file| fixture.host_directory().join(file));
    assert!(files.iter().all(|file| file.exists()));

    fixture.host.remove("test.git").await.unwrap();

    assert!(files.iter().all(|file| !file.exists()), "{files:?}");
    assert_eq!(fixture.host.extensions(), Vec::new());
    assert_eq!(fixture.reopen().extensions(), Vec::new());
    let refused = fixture.text("test.git", "echo", "no").await;
    assert_eq!(refused, Err(CallError::NotInstalled));
    let again = fixture.host.remove("test.git").await;
    assert!(matches!(again, Err(StateError::NotInstalled)), "{again:?}");

    // The identifier is free: a new installation starts a new process with
    // the permissions of that installation.
    fixture.install("test.git");
    let new_process = fixture.process_info("test.git").await;
    assert_ne!(new_process.pid, process.pid);
    assert_eq!(new_process.scopes, Vec::<String>::new());
    fixture.host.shutdown().await;
}
