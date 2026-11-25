//! The configuration of the SSH client of the user: `~/.ssh/config`.
//!
//! [`resolve`] reads the file as OpenSSH does and gives the settings of one
//! host. `docs/remote.md` lists what this client supports. An option that
//! applies to the host and that this client does not have is an error: the
//! client does not connect in a way that the configuration does not
//! describe.

mod lexer;
mod options;
mod settings;
mod tokens;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use self::lexer::parse_line;
use self::options::{Location, Options};
pub use self::settings::{HostSettings, IdentityAgent, StrictHostKeyChecking};
use super::pattern::{matches, matches_list};
use super::{SshEnvironment, SshError, SshTarget};

/// OpenSSH permits 16 levels of `Include`.
const MAX_INCLUDE_DEPTH: usize = 16;

/// Gives the settings of the connection to `target`: the configuration of
/// the user, then the defaults of OpenSSH.
///
/// # Errors
///
/// Returns an error if a file cannot be read or has a line that is not
/// valid, and if the configuration of the host needs an option that this
/// client does not support.
pub fn resolve(target: &SshTarget, environment: &SshEnvironment) -> Result<HostSettings, SshError> {
    let file = environment.ssh_directory().join("config");
    let mut reader = Reader {
        target,
        environment,
        options: Options::default(),
    };
    reader.read_file(&file, 0)?;
    reader.options.finish(target, environment, &file)
}

/// Reads the configuration files for one host.
struct Reader<'a> {
    target: &'a SshTarget,
    environment: &'a SshEnvironment,
    options: Options,
}

impl Reader<'_> {
    /// Reads one file. The lines before the first `Host` or `Match` line
    /// are active for each host. A file that does not exist sets nothing.
    fn read_file(&mut self, file: &Path, depth: usize) -> Result<(), SshError> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(SshError::Configuration {
                    file: file.to_owned(),
                    line: 0,
                    problem: format!("cannot be read: {error}"),
                });
            }
        };
        let mut active = true;
        for (index, line) in text.lines().enumerate() {
            let location = Location {
                file,
                line: index + 1,
            };
            let Some(directive) = parse_line(line).map_err(|problem| location.error(problem))?
            else {
                continue;
            };
            match directive.keyword.as_str() {
                "host" => {
                    let patterns = directive.arguments.iter().map(String::as_str);
                    active = matches_list(patterns, &self.target.host);
                }
                "match" => active = self.match_applies(&directive.arguments, location)?,
                "include" if active => {
                    for pattern in &directive.arguments {
                        self.include(pattern, location, depth)?;
                    }
                }
                _ if active => self.options.apply(&directive, location)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// Reads the files that one argument of `Include` names. A path that is
    /// not absolute is relative to `~/.ssh`.
    fn include(
        &mut self,
        pattern: &str,
        location: Location<'_>,
        depth: usize,
    ) -> Result<(), SshError> {
        if depth >= MAX_INCLUDE_DEPTH {
            return Err(location.error("too many levels of `Include`"));
        }
        let home = self.environment.home.to_string_lossy();
        let path = match pattern.strip_prefix("~/") {
            Some(rest) => PathBuf::from(format!("{home}/{rest}")),
            None => self.environment.ssh_directory().join(pattern),
        };
        for file in included_files(&path, location)? {
            self.read_file(&file, depth + 1)?;
        }
        Ok(())
    }

    /// Evaluates the criteria of a `Match` line. All criteria must be true.
    /// A criterion that this client does not have is an error only if the
    /// criteria before it are true: the line can apply to the host then.
    fn match_applies(
        &self,
        arguments: &[String],
        location: Location<'_>,
    ) -> Result<bool, SshError> {
        let host_name = self.options.host_name().unwrap_or(&self.target.host);
        let user = self
            .target
            .user
            .as_deref()
            .or(self.options.user())
            .unwrap_or(&self.environment.user);
        let mut arguments = arguments.iter();
        while let Some(criterion) = arguments.next() {
            let (negated, name) = match criterion.strip_prefix('!') {
                Some(name) => (true, name),
                None => (false, criterion.as_str()),
            };
            let subject = match name.to_ascii_lowercase().as_str() {
                "all" => {
                    continue;
                }
                "host" => host_name,
                "originalhost" => &self.target.host,
                "user" => user,
                "localuser" => &self.environment.user,
                _ => {
                    return Err(SshError::UnsupportedOption {
                        option: format!("Match {name}"),
                        file: location.file.to_owned(),
                        line: location.line,
                    });
                }
            };
            let patterns = arguments
                .next()
                .ok_or_else(|| location.error(format!("`Match {name}` needs a value")))?;
            if matches_list(patterns.split(','), subject) == negated {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The files that match an `Include` path, in the sequence of their names.
/// Only the last part of the path can have a wildcard.
fn included_files(path: &Path, location: Location<'_>) -> Result<Vec<PathBuf>, SshError> {
    let has_wildcard = |part: &str| part.contains(['*', '?']);
    let name = path
        .file_name()
        .map_or_default(|name| name.to_string_lossy().into_owned());
    if !has_wildcard(&name) {
        return Ok(vec![path.to_owned()]);
    }
    let directory = path.parent().unwrap_or(Path::new("/"));
    if has_wildcard(&directory.to_string_lossy()) {
        return Err(SshError::UnsupportedOption {
            option: "Include with a wildcard in a directory name".to_owned(),
            file: location.file.to_owned(),
            line: location.line,
        });
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(
                location.error(format!("`{}` cannot be read: {error}", directory.display()))
            );
        }
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| matches(&name, &entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A home directory with `~/.ssh/config`.
    struct Home {
        directory: tempfile::TempDir,
    }

    impl Home {
        fn with_config(config: &str) -> Self {
            let home = Self {
                directory: tempfile::tempdir().unwrap(),
            };
            home.write(".ssh/config", config);
            home
        }

        fn write(&self, path: &str, text: &str) {
            let path = self.directory.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn path(&self, path: &str) -> PathBuf {
            self.directory.path().join(path)
        }

        fn resolve(&self, target: &str) -> Result<HostSettings, SshError> {
            let environment = SshEnvironment {
                home: self.directory.path().to_owned(),
                user: "ada".to_owned(),
                agent_socket: None,
            };
            resolve(&target.parse().unwrap(), &environment)
        }
    }

    #[test]
    fn without_a_configuration_the_defaults_of_openssh_apply() {
        let home = Home {
            directory: tempfile::tempdir().unwrap(),
        };

        let settings = home.resolve("vps.example.com").unwrap();

        assert_eq!(settings.host_name, "vps.example.com");
        assert_eq!((settings.port, settings.user.as_str()), (22, "ada"));
        assert_eq!(settings.host_key_alias, "vps.example.com");
        assert_eq!(settings.identity_files[0], home.path(".ssh/id_rsa"));
        assert_eq!(settings.identity_files[3], home.path(".ssh/id_ed25519"));
        assert_eq!(
            settings.user_known_hosts_files[0],
            home.path(".ssh/known_hosts")
        );
        assert_eq!(
            settings.strict_host_key_checking,
            StrictHostKeyChecking::Strict
        );
        assert_eq!(settings.identity_agent, IdentityAgent::Environment);
        assert!(!settings.identities_only);
        assert_eq!(settings.proxy_jump, []);
    }

    #[test]
    fn the_first_value_of_an_option_wins_and_identity_files_add() {
        let home = Home::with_config(
            "Host vps\n\
             \tHostName %h.example.com\n\
             \tUser deploy\n\
             \tPort 2222\n\
             \tIdentityFile ~/.ssh/first\n\
             Host *\n\
             \tUser everyone\n\
             \tPort 22\n\
             \tIdentityFile %d/.ssh/second_%r\n\
             \tIdentitiesOnly yes\n\
             \tStrictHostKeyChecking accept-new\n\
             \tUserKnownHostsFile ~/.ssh/hosts_a \"~/.ssh/hosts b\"\n\
             \tServerAliveInterval 0\n\
             \tConnectTimeout 7\n",
        );

        let settings = home.resolve("vps").unwrap();

        assert_eq!(settings.host_name, "vps.example.com");
        assert_eq!((settings.port, settings.user.as_str()), (2222, "deploy"));
        assert_eq!(
            settings.identity_files,
            [home.path(".ssh/first"), home.path(".ssh/second_deploy")]
        );
        assert!(settings.identities_only);
        assert_eq!(
            settings.strict_host_key_checking,
            StrictHostKeyChecking::AcceptNew
        );
        assert_eq!(
            settings.user_known_hosts_files,
            [home.path(".ssh/hosts_a"), home.path(".ssh/hosts b")]
        );
        assert_eq!(settings.server_alive_interval, None);
        assert_eq!(settings.connect_timeout, Duration::from_secs(7));

        // A different host gets only the `Host *` block.
        let other = home.resolve("other").unwrap();
        assert_eq!((other.port, other.user.as_str()), (22, "everyone"));
    }

    #[test]
    fn the_user_and_the_port_of_the_destination_come_before_the_configuration() {
        let home = Home::with_config("Host vps\n  User deploy\n  Port 2222\n");

        let settings = home.resolve("ssh://root@vps:2200").unwrap();

        assert_eq!((settings.port, settings.user.as_str()), (2200, "root"));
    }

    #[test]
    fn host_patterns_with_a_negation_select_the_block() {
        let home = Home::with_config(
            "Host *.example.com !db.example.com\n  User web\nHost db.example.com\n  User database\n",
        );

        assert_eq!(home.resolve("a.example.com").unwrap().user, "web");
        assert_eq!(home.resolve("db.example.com").unwrap().user, "database");
    }

    #[test]
    fn an_include_is_read_at_its_position_and_only_in_an_active_block() {
        let home = Home::with_config(
            "Include config.d/*.conf\n\
             Host vps\n\
             \tInclude only-vps\n\
             Host *\n\
             \tUser last\n",
        );
        home.write(".ssh/config.d/10-first.conf", "Host vps\n  Port 2222\n");
        home.write(
            ".ssh/config.d/20-second.conf",
            "Host vps\n  Port 3333\n  User second\n",
        );
        home.write(".ssh/config.d/ignored.txt", "Host vps\n  Port 4444\n");
        home.write(".ssh/only-vps", "HostKeyAlias alias-of-vps\n");

        let vps = home.resolve("vps").unwrap();
        assert_eq!((vps.port, vps.user.as_str()), (2222, "second"));
        assert_eq!(vps.host_key_alias, "alias-of-vps");

        let other = home.resolve("other").unwrap();
        assert_eq!(other.user, "last");
        assert_eq!(other.host_key_alias, "other");
    }

    #[test]
    fn a_proxy_jump_list_is_read_and_none_removes_it() {
        let home = Home::with_config(
            "Host direct\n  ProxyJump none\nHost *\n  ProxyJump first,deploy@second:2222\n",
        );

        let jumps = home.resolve("inner").unwrap().proxy_jump;
        assert_eq!(jumps.len(), 2);
        assert_eq!(jumps[0].host, "first");
        assert_eq!(
            (
                jumps[1].user.as_deref(),
                jumps[1].host.as_str(),
                jumps[1].port
            ),
            (Some("deploy"), "second", Some(2222))
        );

        assert_eq!(home.resolve("direct").unwrap().proxy_jump, []);
    }

    #[test]
    fn an_unsupported_option_fails_only_for_the_host_that_it_applies_to() {
        let home = Home::with_config(
            "Host hop\n  ProxyCommand ssh -W %h:%p bastion\nHost cert\n  CertificateFile ~/.ssh/cert\n",
        );

        let error = home.resolve("hop").unwrap_err();
        let SshError::UnsupportedOption { option, line, .. } = &error else {
            panic!("{error:?}");
        };
        assert_eq!((option.as_str(), *line), ("ProxyCommand", 2));
        assert!(error.to_string().contains("ProxyCommand"));

        assert!(matches!(
            home.resolve("cert"),
            Err(SshError::UnsupportedOption { option, .. }) if option == "CertificateFile"
        ));
        assert!(home.resolve("other").is_ok());
    }

    #[test]
    fn a_match_line_applies_by_host_and_user_and_fails_for_exec_only_when_reached() {
        let home = Home::with_config(
            "Host vps\n\
             \tHostName vps.example.com\n\
             Match host *.example.com user deploy\n\
             \tPort 2222\n\
             Match host other.invalid exec \"test -f x\"\n\
             \tPort 1\n\
             Match all\n\
             \tIdentityAgent none\n",
        );

        assert_eq!(home.resolve("deploy@vps").unwrap().port, 2222);
        let as_ada = home.resolve("vps").unwrap();
        assert_eq!(as_ada.port, 22);
        assert_eq!(as_ada.identity_agent, IdentityAgent::Disabled);

        let reached = Home::with_config("Match host vps exec \"test -f x\"\n  Port 1\n");
        assert!(matches!(
            reached.resolve("vps"),
            Err(SshError::UnsupportedOption { option, .. }) if option == "Match exec"
        ));
    }

    #[test]
    fn an_option_that_changes_nothing_for_nomux_is_ignored() {
        let home = Home::with_config(
            "Host *\n  ForwardAgent yes\n  ControlMaster auto\n  LocalForward 8080 localhost:80\n  SendEnv LANG\n",
        );

        assert!(home.resolve("vps").is_ok());
    }

    #[test]
    fn a_line_that_is_not_valid_names_the_file_and_the_line() {
        let home = Home::with_config("Host vps\n  Port many\n");

        let message = home.resolve("vps").unwrap_err().to_string();

        assert!(message.contains(".ssh/config line 2"), "{message}");
    }
}
