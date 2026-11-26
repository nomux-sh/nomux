//! The session commands: `new`, `attach`, `start`, `capture`,
//! `restore-policy`, `detach`, `kill` and `rename`. Each command that takes
//! a session finds it by its target first: `Client::find_session` has the
//! rule.

mod arguments;

use std::env;
use std::path::{self, PathBuf};

use nomux_client::{Client, ClientError, Host, NewSession, RestorePolicy, Session};
use nomux_core::configuration::PrefixKeys;

pub(crate) use self::arguments::{NewArguments, RenameArguments, Target};
use crate::attach::{self, PrefixOption};
use crate::commands::KIND;
use crate::report::Report;

/// `new`: starts a session, or with `-A` finds the session of the name, and
/// attaches with `-a` and `-A`.
pub(crate) async fn new(
    host: &Host,
    host_name: Option<&str>,
    arguments: NewArguments,
) -> Result<Report, ClientError> {
    // A setting of the keys that is not valid starts no session.
    let prefixes = arguments
        .attaches()
        .then(|| arguments.prefix.prefix_keys())
        .transpose()?;
    let command = arguments.command();
    let gave_start = !command.is_empty() || arguments.cwd.is_some();
    let session = NewSession {
        command,
        working_directory: working_directory(arguments.cwd.clone(), host_name.is_some())?,
        name: arguments.name.clone(),
        reuse_existing: arguments.attach_or_create,
    };
    let mut client = host.client(KIND).await?;
    let (session, created) = client.open_session(session).await?;
    if !created && gave_start {
        eprintln!(
            "nomux: session {} exists: the command and --cwd are not used",
            session.label()
        );
    }
    let Some(prefixes) = prefixes else {
        return Ok(Report::SessionCreated(session));
    };
    let created = arguments.attach_or_create.then_some(created);
    attach_to(host, host_name, client, session, prefixes, created).await
}

pub(crate) async fn attach(
    host: &Host,
    host_name: Option<&str>,
    target: &Target,
    prefix: &PrefixOption,
) -> Result<Report, ClientError> {
    let prefixes = prefix.prefix_keys()?;
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    attach_to(host, host_name, client, session, prefixes, None).await
}

pub(crate) async fn detach(host: &Host, target: &Target) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    let clients = client.detach_session(&session.id).await?;
    Ok(Report::ClientsDetached { session, clients })
}

pub(crate) async fn start(host: &Host, target: &Target) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    let session = client.start_session(&session.id).await?;
    Ok(Report::SessionStarted(session))
}

pub(crate) async fn capture(
    host: &Host,
    target: &Target,
    lines: usize,
) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let id = client.find_session(target.text()).await?.id;
    let output = client.capture_lines(&id, lines).await?;
    Ok(Report::OutputCaptured { id, output })
}

pub(crate) async fn set_restore_policy(
    host: &Host,
    target: &str,
    policy: RestorePolicy,
) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target).await?;
    client.set_restore_policy(&session.id, policy).await?;
    Ok(Report::RestorePolicySet { session, policy })
}

/// A session lives without a daemon, thus `kill` starts one too.
pub(crate) async fn kill(host: &Host, target: &Target) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    client.kill_session(&session.id).await?;
    Ok(Report::SessionKilled { session })
}

pub(crate) async fn rename(
    host: &Host,
    arguments: &RenameArguments,
) -> Result<Report, ClientError> {
    let Some((target, name)) = arguments.target_and_name() else {
        return Err(ClientError::Failed(
            "rename needs the session and its new name: `nomux rename <target> <new-name>`"
                .to_owned(),
        ));
    };
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target).await?;
    let renamed = client.rename_session(&session.id, Some(name)).await?;
    Ok(Report::SessionRenamed {
        session: renamed,
        previous_name: session.name,
    })
}

/// Attaches to `session`. `client` becomes the connection for the window
/// size of the attach.
async fn attach_to(
    host: &Host,
    host_name: Option<&str>,
    client: Client,
    session: Session,
    prefixes: PrefixKeys,
    created: Option<bool>,
) -> Result<Report, ClientError> {
    let end = attach::run(host, host_name, client, &session, prefixes).await?;
    Ok(Report::AttachEnded {
        session,
        end,
        created,
    })
}

/// The absolute working directory of a new session. On the local host:
/// `--cwd`, or the current directory of the CLI. On a remote host: `--cwd`,
/// which must be an absolute path there, or `None` for the home directory.
fn working_directory(
    requested: Option<PathBuf>,
    on_remote_host: bool,
) -> Result<Option<PathBuf>, ClientError> {
    let directory = match (requested, on_remote_host) {
        (Some(directory), true) if directory.is_absolute() => return Ok(Some(directory)),
        (Some(directory), true) => {
            return Err(ClientError::Failed(format!(
                "with --host, --cwd must be an absolute path on the host: {} is not absolute",
                directory.display()
            )));
        }
        (None, true) => return Ok(None),
        (Some(directory), false) => path::absolute(directory),
        (None, false) => env::current_dir(),
    };
    directory.map(Some).map_err(|error| {
        ClientError::Failed(format!("the working directory is not valid: {error}"))
    })
}
