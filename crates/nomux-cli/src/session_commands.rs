//! The session commands: `new`, `attach`, `start`, `capture`,
//! `restore-policy`, `detach`, `kill`, `rename`, `send`, `hibernate`,
//! `wake`, `pin` and `unpin`. Each command that takes a session finds it by
//! its target first: `Client::find_session` has the rule.

mod arguments;
mod follow;
mod key_names;
mod kill_all;
mod send;

use std::env;
use std::path::{self, PathBuf};

use nomux_client::{Client, ClientError, Host, NewSession, RestorePolicy, Session};

pub(crate) use self::arguments::{
    AttachArguments, CaptureArguments, KillArguments, NewArguments, RenameArguments, SendArguments,
    Target,
};
pub(crate) use self::send::send;
use crate::attach::{self, AttachMode};
use crate::commands::KIND;
use crate::output::Format;
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
        sandbox: arguments.sandbox.clone(),
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
    let mode = AttachMode {
        prefixes,
        read_only: false,
    };
    attach_to(host, host_name, client, session, mode, created).await
}

/// `attach`, and bare `nomux` on a terminal.
pub(crate) async fn attach(
    host: &Host,
    host_name: Option<&str>,
    arguments: &AttachArguments,
) -> Result<Report, ClientError> {
    let prefixes = arguments.prefix.prefix_keys()?;
    let mut client = host.client(KIND).await?;
    let session = match arguments.target.text() {
        Some(target) => client.find_session(target).await?,
        None => match client.most_recent_session().await? {
            Some(session) => session,
            None if arguments.start_if_none => {
                client
                    .create_session(NewSession {
                        working_directory: working_directory(None, host_name.is_some())?,
                        ..NewSession::default()
                    })
                    .await?
            }
            None => return Err(ClientError::NotFound("no sessions".to_owned())),
        },
    };
    if arguments.detach_others {
        client.detach_session(&session.id).await?;
    }
    let mode = AttachMode {
        prefixes,
        read_only: arguments.read_only,
    };
    attach_to(host, host_name, client, session, mode, None).await
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
    arguments: &CaptureArguments,
    format: Format,
) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let id = client.find_session(arguments.target.text()).await?.id;
    if arguments.follow {
        if format == Format::Json {
            return Err(ClientError::Failed(
                "capture --follow has no JSON form: its output is the bytes of the command"
                    .to_owned(),
            ));
        }
        return follow::follow(host, client, &id, arguments.lines).await;
    }
    let output = client.capture_lines(&id, arguments.lines).await?;
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

pub(crate) async fn hibernate(host: &Host, target: &Target) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    let session = client.hibernate_session(&session.id).await?;
    Ok(Report::SessionHibernated(session))
}

/// Starts the resume command of a hibernated session. A session that is
/// not hibernated is not started: `start` does that.
pub(crate) async fn wake(host: &Host, target: &Target) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    if !session.is_hibernated() {
        return Err(ClientError::Failed(format!(
            "session {} is not hibernated: `nomux start {}` starts a dormant session",
            session.label(),
            target.text()
        )));
    }
    let session = client.start_session(&session.id).await?;
    Ok(Report::SessionWoken(session))
}

pub(crate) async fn set_pinned(
    host: &Host,
    target: &Target,
    pinned: bool,
) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    let session = client.find_session(target.text()).await?;
    client.set_pinned(&session.id, pinned).await?;
    Ok(Report::SessionPinned { session, pinned })
}

/// A session lives without a daemon, thus `kill` starts one too.
pub(crate) async fn kill(host: &Host, arguments: &KillArguments) -> Result<Report, ClientError> {
    let mut client = host.client(KIND).await?;
    if arguments.all {
        return kill_all::kill_all(client, &arguments.all_options).await;
    }
    let Some(target) = arguments.target.text() else {
        return Err(ClientError::Failed(
            "kill needs a session, or --all: `nomux kill <target>`".to_owned(),
        ));
    };
    let session = client.find_session(target).await?;
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
    mode: AttachMode,
    created: Option<bool>,
) -> Result<Report, ClientError> {
    let end = attach::run(host, host_name, client, &session, mode).await?;
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
