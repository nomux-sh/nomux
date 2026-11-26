//! The checks of a remote host: the connection, the platform, the binaries,
//! the permissions, the daemon and the versions.
//!
//! The checks change nothing on the host: they install no binaries and
//! start no daemon.

mod permissions;
mod worktrees;

use nomux_transport::ssh::{SshConnection, SshEnvironment, SshError, SshTarget};

use self::permissions::check_permissions;
use crate::bootstrap::{BinarySource, RemotePlatform, VERSION, daemon_socket, probe, remote_shell};
use crate::ssh_host::daemon_process_runs;
use crate::{Client, ClientError, ClientKind, Host, HostInfo};

/// The result of one check of a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCheck {
    /// The name of the check: `connection`, `platform`, `binaries`,
    /// `permissions`, `daemon`, `versions`, `memory-limit`, `free-space` or
    /// `shared-build-output`.
    pub name: &'static str,
    /// `true` if the host passed the check.
    pub passed: bool,
    /// What the check found. For a check that failed, the reason and what
    /// to do.
    pub detail: String,
}

impl HostCheck {
    fn of(name: &'static str, result: Result<String, String>) -> Self {
        let passed = result.is_ok();
        let (Ok(detail) | Err(detail)) = result;
        Self {
            name,
            passed,
            detail,
        }
    }
}

/// Checks the remote host `ssh_target`. A check that cannot run after a
/// failure is not in the result: without a connection, the result has only
/// the `connection` check.
pub async fn check_host(
    ssh_target: &str,
    binaries: &BinarySource,
    kind: ClientKind,
) -> Vec<HostCheck> {
    let mut checks = Vec::new();
    let connection = match connect(ssh_target).await {
        Ok(connection) => connection,
        Err(error) => {
            checks.push(HostCheck::of("connection", Err(error.to_string())));
            return checks;
        }
    };
    checks.push(HostCheck::of(
        "connection",
        Ok(format!(
            "{}, host key {}",
            connection.description(),
            connection.host_key()
        )),
    ));

    let label = match binaries.label() {
        Ok(label) => label,
        Err(error) => {
            checks.push(HostCheck::of("binaries", Err(error.to_string())));
            return checks;
        }
    };
    let probe = match probe(&connection, &label).await {
        Ok(probe) => probe,
        Err(error) => {
            checks.push(HostCheck::of("platform", Err(error.to_string())));
            return checks;
        }
    };
    let platform = RemotePlatform::from_uname(&probe.system, &probe.machine)
        .map(|platform| format!("{} {}: {}", probe.system, probe.machine, platform.target))
        .map_err(|error| error.to_string());
    checks.push(HostCheck::of("platform", platform));

    let installed = if probe.installed {
        Ok(format!("nomux {VERSION} is in ~/.nomux/bin/{label}"))
    } else {
        Err(format!(
            "nomux {VERSION} is not in ~/.nomux/bin/{label}. Run `nomux host add` for this host"
        ))
    };
    checks.push(HostCheck::of("binaries", installed));
    checks.push(HostCheck::of("permissions", permissions(&connection).await));

    let socket = daemon_socket(&probe.home);
    let (daemon, versions, memory_limit, worktrees) = daemon(&connection, &socket, kind).await;
    checks.push(HostCheck::of("daemon", daemon));
    checks.extend(versions.map(|versions| HostCheck::of("versions", versions)));
    // A host without a limit of the operating system still works: the
    // limit is then a threshold of the hibernation.
    checks.extend(memory_limit.map(|detail| HostCheck::of("memory-limit", Ok(detail))));
    checks.extend(worktrees);
    checks
}

/// Checks the local host: the daemon and the worktrees of its Projects. The
/// check starts no daemon.
pub async fn check_local_host(kind: ClientKind) -> Vec<HostCheck> {
    let client = match Host::local() {
        Ok(host) => host.running_client(kind).await,
        Err(error) => Err(error),
    };
    let mut client = match client {
        Ok(Some(client)) => client,
        Ok(None) => {
            let detail = "no daemon runs. The next command starts it".to_owned();
            return vec![HostCheck::of("daemon", Ok(detail))];
        }
        Err(error) => return vec![HostCheck::of("daemon", Err(error.to_string()))],
    };
    let mut checks = vec![HostCheck::of(
        "daemon",
        client
            .host_info()
            .await
            .map(|info| daemon_detail(&info))
            .map_err(|error| error.to_string()),
    )];
    checks.extend(worktree_checks(&mut client).await);
    checks
}

/// The checks of the worktrees. None if the daemon does not answer
/// `Worktree.List`, for example a daemon of an older version.
async fn worktree_checks(client: &mut Client) -> Vec<HostCheck> {
    match client.worktrees(None, false).await {
        Ok(projects) => worktrees::checks(&projects),
        Err(_) => Vec::new(),
    }
}

fn daemon_detail(info: &HostInfo) -> String {
    let sessions = match info.session_count {
        1 => "1 session".to_owned(),
        count => format!("{count} sessions"),
    };
    format!(
        "nomuxd {} runs: process ID {}, {sessions}",
        info.daemon_version, info.daemon_pid
    )
}

async fn connect(ssh_target: &str) -> Result<SshConnection, SshError> {
    let target: SshTarget = ssh_target.parse()?;
    SshConnection::connect(&target, &SshEnvironment::of_process()?).await
}

/// Reads the owner and the mode of the directories and of the socket of
/// nomux on the host.
async fn permissions(connection: &SshConnection) -> Result<String, String> {
    let script = "id -u; for path in \"$HOME/.nomux\" \"$HOME/.nomux/bin\" \"$HOME/.nomux/run\" \
                  \"$HOME/.nomux/run/nomuxd.sock\"; do ls -ldn \"$path\" 2>/dev/null || echo \
                  missing; done";
    let output = connection
        .run(&remote_shell(script), &[])
        .await
        .map_err(|error| error.to_string())?;
    check_permissions(&String::from_utf8_lossy(&output.stdout))
}

/// The state of the daemon, and the versions, the memory limit of the
/// operating system and the checks of the worktrees if a daemon answers.
async fn daemon(
    connection: &SshConnection,
    socket: &str,
    kind: ClientKind,
) -> (
    Result<String, String>,
    Option<Result<String, String>>,
    Option<String>,
    Vec<HostCheck>,
) {
    let stream = match connection.open_unix_socket(socket).await {
        Ok(stream) => stream,
        Err(SshError::SocketUnavailable) => {
            let daemon = match daemon_process_runs(connection).await {
                Ok(true) => Err(SshError::ForwardingRefused.to_string()),
                Ok(false) => {
                    Ok("no daemon runs. The next command for this host starts it".to_owned())
                }
                Err(error) => Err(error.to_string()),
            };
            return (daemon, None, None, Vec::new());
        }
        Err(error) => return (Err(error.to_string()), None, None, Vec::new()),
    };
    let mut client = match Client::on(stream, kind).await {
        Ok(client) => client,
        Err(ClientError::VersionIncompatible(message)) => {
            let replace = format!(
                "{message}. The next command of this client for the host replaces the daemon \
                 with nomuxd {VERSION}. No session ends"
            );
            return (
                Ok("a daemon runs".to_owned()),
                Some(Err(replace)),
                None,
                Vec::new(),
            );
        }
        Err(error) => return (Err(error.to_string()), None, None, Vec::new()),
    };
    match client.host_info().await {
        Ok(info) => {
            let daemon = daemon_detail(&info);
            let versions = if info.daemon_version == VERSION {
                format!("the client and the daemon have the version {VERSION}")
            } else {
                format!(
                    "the client has the version {VERSION} and the daemon has the version {}: \
                     they have a common protocol version",
                    info.daemon_version
                )
            };
            let memory_limit = info.memory_limit.map(|limit| limit.detail);
            let worktrees = worktree_checks(&mut client).await;
            (Ok(daemon), Some(Ok(versions)), memory_limit, worktrees)
        }
        Err(error) => (Err(error.to_string()), None, None, Vec::new()),
    }
}
