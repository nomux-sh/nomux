//! Resolves a sandbox profile into the plan of one start of a session: the
//! concrete paths on this host, now. A restore and a wake resolve the
//! profile again, thus a change of the configuration or of the home
//! directory applies at the next start.

use std::path::{Path, PathBuf};

use nomux_core::configuration::{SandboxSettings, UserProfile};
use nomux_holder_protocol::sandbox::SandboxPlan;

use super::builtin_profiles::{
    BUILTIN_PROFILES, BuiltinProfile, OFF, SECRET_PATHS, SYSTEM_READ, SYSTEM_WRITE, TOOL_STATE,
    required_features, wanted_features,
};
use super::home_listing::readable_paths;
use super::project_layout::ProjectLayout;

/// What the plan of a session is made from.
pub(crate) struct PlanInput<'a> {
    pub(crate) profile: &'a str,
    pub(crate) working_directory: &'a Path,
    /// The program of the command: the state of a known agent tool is
    /// permitted when the tool is the command.
    pub(crate) program: &'a Path,
    pub(crate) home: &'a Path,
    /// The temporary directory of the session.
    pub(crate) temporary: &'a Path,
    /// The directory of the agent socket of the daemon.
    pub(crate) agent_socket_directory: &'a Path,
    /// The directory of the binaries of this daemon.
    pub(crate) binary_directory: Option<&'a Path>,
    pub(crate) settings: &'a SandboxSettings,
}

/// The plan of `input`. `None` for the profile `off`.
///
/// # Errors
///
/// Returns the reason, for a person, if the profile does not exist, or if
/// the project directory would give the command the secret paths.
pub(crate) fn resolve(input: &PlanInput<'_>) -> Result<Option<SandboxPlan>, String> {
    if input.profile == OFF {
        return Ok(None);
    }
    let (builtin, user) = find_profile(input.profile, input.settings)?;
    let layout = ProjectLayout::of(input.working_directory, input.home);
    check_project(&layout.directory, input.home)?;

    let tool_state = tool_state(input.program);
    let mut denied: Vec<PathBuf> = SECRET_PATHS
        .iter()
        .filter(|path| !tool_state.contains(path))
        .map(PathBuf::from)
        .collect();
    let mut read: Vec<PathBuf> = SYSTEM_READ.iter().map(PathBuf::from).collect();
    let mut write: Vec<PathBuf> = SYSTEM_WRITE.iter().map(PathBuf::from).collect();
    let mut network_off = builtin.network_off;
    if let Some(user) = user {
        denied.extend(
            user.deny_read
                .iter()
                .filter_map(|path| home_relative(path, input.home)),
        );
        read.extend(user.read.iter().map(|path| expand(path, input.home)));
        write.extend(user.write.iter().map(|path| expand(path, input.home)));
        network_off = user.network_off.unwrap_or(network_off);
    }
    read.extend(
        readable_paths(input.home, &denied)
            .map_err(|error| format!("cannot list the home directory: {error}"))?,
    );
    if builtin.writes_project {
        write.push(layout.directory.clone());
        write.extend(layout.git_write);
    } else {
        read.push(layout.directory.clone());
    }
    read.extend(layout.git_read);
    write.extend(tool_state.iter().map(|path| input.home.join(path)));
    // The binaries of nomux, for `nomux status set` and `nomux hook` in the
    // session.
    read.extend(input.binary_directory.map(Path::to_path_buf));
    // A denied path is never permitted, also not by a rule of the user. A
    // rule cannot take a part of a permitted directory away: a denied path
    // below one is an error.
    let denied_absolute: Vec<PathBuf> = denied.iter().map(|path| input.home.join(path)).collect();
    if let Some(path) = read
        .iter()
        .chain(&write)
        .find(|path| is_below_any(path, &denied_absolute))
    {
        return Err(format!(
            "the sandbox profile {} permits {}, which is a secret path",
            input.profile,
            path.display()
        ));
    }
    check_no_denied_below(read.iter().chain(&write), &denied_absolute)?;

    let mut require = required_features(network_off);
    if input.settings.strict {
        require = require.union(wanted_features());
    }
    Ok(Some(SandboxPlan {
        profile: input.profile.to_owned(),
        read,
        write,
        temporary: Some(input.temporary.to_owned()),
        unix: vec![input.agent_socket_directory.to_owned()],
        network_off,
        require,
        want: wanted_features().without(require),
    }))
}

/// The names of each profile: the built-in ones, then the ones of the user.
pub(crate) fn profile_names(settings: &SandboxSettings) -> Vec<String> {
    std::iter::once(OFF.to_owned())
        .chain(
            BUILTIN_PROFILES
                .iter()
                .map(|profile| profile.name.to_owned()),
        )
        .chain(settings.profiles.keys().cloned())
        .collect()
}

fn find_profile<'a>(
    name: &str,
    settings: &'a SandboxSettings,
) -> Result<(BuiltinProfile, Option<&'a UserProfile>), String> {
    let builtin = |name: &str| {
        BUILTIN_PROFILES
            .iter()
            .find(|profile| profile.name == name)
            .copied()
    };
    if let Some(profile) = builtin(name) {
        return Ok((profile, None));
    }
    let Some(user) = settings.profiles.get(name) else {
        return Err(format!(
            "the sandbox profile {name} does not exist. The profiles are: {}",
            profile_names(settings).join(", ")
        ));
    };
    builtin(&user.extends)
        .map(|profile| (profile, Some(user)))
        .ok_or_else(|| {
            format!(
                "the sandbox profile {name} extends {}, which is not a built-in profile",
                user.extends
            )
        })
}

/// The project directory must not give the command a secret path: it is not
/// the home directory, not above it, and not in a secret path.
fn check_project(project: &Path, home: &Path) -> Result<(), String> {
    let secret = SECRET_PATHS
        .iter()
        .map(|path| home.join(path))
        .find(|secret| project.starts_with(secret));
    if home.starts_with(project) || secret.is_some() {
        return Err(format!(
            "a sandbox cannot use {} as the project directory: the command could then read the \
             secret paths of the home directory. Start the session in a project directory",
            project.display()
        ));
    }
    Ok(())
}

/// The state of the known agent tool that `program` is, relative to the
/// home directory. Empty for each other program.
fn tool_state(program: &Path) -> &'static [&'static str] {
    let name = program.file_name().and_then(|name| name.to_str());
    TOOL_STATE
        .iter()
        .find(|(tool, _)| Some(*tool) == name)
        .map_or(&[], |(_, paths)| paths)
}

fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

/// A path of the user as a path below the home directory. A path outside
/// the home directory has no rule to take away: no rule permits it.
fn home_relative(path: &str, home: &Path) -> Option<PathBuf> {
    expand(path, home)
        .strip_prefix(home)
        .ok()
        .map(Path::to_path_buf)
}

fn is_below_any(path: &Path, denied: &[PathBuf]) -> bool {
    denied.iter().any(|denied| path.starts_with(denied))
}

fn check_no_denied_below<'a>(
    permitted: impl Iterator<Item = &'a PathBuf>,
    denied: &[PathBuf],
) -> Result<(), String> {
    for path in permitted {
        if let Some(inside) = denied
            .iter()
            .find(|denied| denied.starts_with(path) && denied.exists())
        {
            return Err(format!(
                "the sandbox cannot deny {} and permit {}: a rule cannot take a part of a                  directory away",
                inside.display(),
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nomux_holder_protocol::sandbox::{FeatureSet, SandboxFeature};

    use super::*;

    struct Home {
        directory: tempfile::TempDir,
    }

    impl Home {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            for path in [
                ".ssh",
                ".config/gh",
                ".config/git",
                "src/api/.git",
                ".claude",
            ] {
                fs::create_dir_all(directory.path().join(path)).unwrap();
            }
            Self { directory }
        }

        fn path(&self) -> &Path {
            self.directory.path()
        }

        fn plan(
            &self,
            profile: &str,
            working_directory: &str,
            program: &str,
            settings: &SandboxSettings,
        ) -> Result<Option<SandboxPlan>, String> {
            resolve(&PlanInput {
                profile,
                working_directory: &self.path().join(working_directory),
                program: Path::new(program),
                home: self.path(),
                temporary: Path::new("/state/tmp"),
                agent_socket_directory: Path::new("/run/nomuxd.agent"),
                binary_directory: None,
                settings,
            })
        }
    }

    #[test]
    fn each_profile_gives_its_rules() {
        let home = Home::new();
        let settings = SandboxSettings::default();
        let project = home.path().join("src/api");
        let files = FeatureSet::EMPTY.with(SandboxFeature::Files);
        let offline = files.with(SandboxFeature::NetworkOff);
        // Profile, writes the project, network off, required features.
        let table = [
            ("project", true, false, files),
            ("project-offline", true, true, offline),
            ("read-only", false, true, offline),
        ];
        for (profile, writes_project, network_off, require) in table {
            let plan = home
                .plan(profile, "src/api/module", "sh", &settings)
                .unwrap()
                .unwrap();
            assert_eq!(plan.write.contains(&project), writes_project, "{profile}");
            assert_eq!(plan.read.contains(&project), !writes_project, "{profile}");
            assert_eq!(plan.network_off, network_off, "{profile}");
            assert_eq!(plan.require, require, "{profile}");
            assert_eq!(plan.temporary.as_deref(), Some(Path::new("/state/tmp")));
            assert_eq!(plan.unix, [PathBuf::from("/run/nomuxd.agent")]);
            assert!(
                plan.read.contains(&home.path().join(".config/git")),
                "{profile}"
            );
            for secret in [".ssh", ".config/gh", ".config", ".claude"] {
                let secret = home.path().join(secret);
                assert!(
                    !plan.read.contains(&secret) && !plan.write.contains(&secret),
                    "{profile}"
                );
            }
        }
        assert_eq!(home.plan("off", "src/api", "sh", &settings), Ok(None));
    }

    #[test]
    fn the_agent_tool_gets_its_state_and_a_shell_does_not() {
        let home = Home::new();
        let settings = SandboxSettings::default();
        let state = home.path().join(".claude");

        let tool = home.plan("project", "src/api", "/usr/bin/claude", &settings);
        let shell = home.plan("project", "src/api", "/bin/sh", &settings);

        assert!(tool.unwrap().unwrap().write.contains(&state));
        assert!(!shell.unwrap().unwrap().write.contains(&state));
    }

    #[test]
    fn a_user_profile_adds_rules_and_never_permits_a_secret() {
        let home = Home::new();
        let mut settings = SandboxSettings::default();
        let user = |write: &str| UserProfile {
            extends: "project".to_owned(),
            write: vec![write.to_owned()],
            deny_read: vec!["~/src/api/private".to_owned()],
            network_off: Some(true),
            ..UserProfile::default()
        };
        settings
            .profiles
            .insert("mine".to_owned(), user("~/scratch"));
        settings.profiles.insert("leaky".to_owned(), user("~/.ssh"));
        fs::create_dir_all(home.path().join("src/api/private")).unwrap();

        let mine = home.plan("mine", "src/api", "sh", &settings);
        let leaky = home.plan("leaky", "src/api", "sh", &settings);

        // A denied path below the project cannot be taken out of it.
        assert!(mine.unwrap_err().contains("cannot deny"));
        assert!(leaky.unwrap_err().contains("secret path"));
        fs::remove_dir(home.path().join("src/api/private")).unwrap();
        let mine = home
            .plan("mine", "src/api", "sh", &settings)
            .unwrap()
            .unwrap();
        assert!(mine.write.contains(&home.path().join("scratch")));
        assert!(mine.network_off);
    }

    #[test]
    fn the_home_directory_and_a_secret_path_are_not_a_project() {
        let home = Home::new();
        let settings = SandboxSettings::default();

        for directory in ["", ".ssh"] {
            let refused = home.plan("project", directory, "sh", &settings);
            assert!(
                refused.unwrap_err().contains("project directory"),
                "{directory:?}"
            );
        }
        assert!(
            home.plan("nosuch", "src/api", "sh", &settings)
                .unwrap_err()
                .contains("does not exist")
        );
    }

    #[test]
    fn strict_settings_require_each_wanted_feature() {
        let home = Home::new();
        let settings = SandboxSettings {
            strict: true,
            ..SandboxSettings::default()
        };

        let plan = home
            .plan("project", "src/api", "sh", &settings)
            .unwrap()
            .unwrap();

        assert!(plan.require.contains(SandboxFeature::UnixSockets));
        assert!(plan.want.is_empty());
    }
}
