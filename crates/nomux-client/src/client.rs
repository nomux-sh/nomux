//! One connection to a daemon.

use std::io;
use std::path::Path;
use std::time::Duration;

use nomux_proto::capabilities::{OUTPUT_HISTORY, SESSION_ACTIVITY, SESSION_NAMES};
use nomux_proto::compatibility::{VersionRange, negotiate};
use nomux_proto::wire::v1::{
    self as wire, Envelope, Hello, HostInfoRequest, HostStopRequest, Request, SessionAttachRequest,
    SessionCaptureRequest, SessionCreateRequest, SessionDetachRequest, SessionInputRequest,
    SessionKillRequest, SessionListRequest, SessionRenameRequest, SessionResizeRequest,
    SessionSetRestorePolicyRequest, SessionStartRequest, envelope, request, response,
};
use nomux_transport::{EnvelopeStream, FrameReader, FrameWriter};
use tokio::time::timeout;

use crate::session::wire_restore_policy;
use crate::terminal::{AttachOptions, TerminalInput, TerminalOutput};
use crate::{
    CaptureRange, CapturedOutput, ClientError, ClientKind, HostInfo, NewSession, RestorePolicy,
    Session, WindowSize,
};

/// The longest time that a client waits for the response to one request.
/// The daemon ends each request of this protocol version earlier.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A connection to a daemon on which the handshake is complete.
///
/// Each method sends one request and waits for its response.
/// [`Host::client`](crate::Host::client) makes a connection to the daemon of
/// the local host or of a remote host.
#[derive(Debug)]
pub struct Client {
    stream: EnvelopeStream,
    next_request_id: u64,
    /// The capability flags of the daemon.
    daemon_capabilities: Vec<String>,
}

impl Client {
    /// Connects to the daemon that listens on the Unix socket `socket` and
    /// does the handshake.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::DaemonNotRunning`] if no daemon listens on
    /// the socket, [`ClientError::PermissionDenied`] if the socket belongs
    /// to a different user, and [`ClientError::VersionIncompatible`] if the
    /// client and the daemon have no common protocol version.
    pub async fn connect(socket: &Path, kind: ClientKind) -> Result<Self, ClientError> {
        let stream =
            nomux_transport::connect(socket)
                .await
                .map_err(|error| match error.kind() {
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                        ClientError::DaemonNotRunning
                    }
                    io::ErrorKind::PermissionDenied => ClientError::PermissionDenied(format!(
                        "no permission to use the daemon socket {}",
                        socket.display()
                    )),
                    _ => ClientError::Connection(error),
                })?;
        Self::on(stream, kind).await
    }

    /// Does the handshake on a connection to a daemon.
    pub(crate) async fn on(stream: EnvelopeStream, kind: ClientKind) -> Result<Self, ClientError> {
        let mut client = Self {
            stream,
            next_request_id: 1,
            daemon_capabilities: Vec::new(),
        };
        client.handshake(kind).await?;
        Ok(client)
    }

    async fn handshake(&mut self, kind: ClientKind) -> Result<(), ClientError> {
        let local = VersionRange::CURRENT;
        let hello = Hello {
            version: local.version,
            oldest: local.oldest,
            // This client reads the output of an earlier epoch and the
            // frames that come with it, and the names and the activity of
            // the sessions.
            capabilities: [OUTPUT_HISTORY, SESSION_NAMES, SESSION_ACTIVITY]
                .map(str::to_owned)
                .to_vec(),
            client_kind: wire::ClientKind::from(kind).into(),
            token: String::new(),
        };
        let response::Result::Welcome(welcome) = self.call(request::Body::Hello(hello)).await?
        else {
            return Err(unexpected_result());
        };
        let daemon = VersionRange {
            version: welcome.version,
            oldest: welcome.oldest,
        };
        negotiate(local, daemon).map_err(|_| {
            ClientError::VersionIncompatible(format!(
                "the client and the daemon have no common protocol version: the client supports \
                 {} to {}, the daemon supports {} to {}",
                local.oldest, local.version, daemon.oldest, daemon.version
            ))
        })?;
        self.daemon_capabilities = welcome.capabilities;
        Ok(())
    }

    /// Fails if the daemon does not have the capability flag `flag`: it is
    /// older than the `feature`.
    fn require(&self, flag: &str, feature: &str) -> Result<(), ClientError> {
        if self.daemon_capabilities.iter().any(|known| known == flag) {
            Ok(())
        } else {
            Err(ClientError::Failed(format!(
                "the daemon is older than {feature}: update nomux on the host and stop the                  daemon with `nomux daemon stop`, then run the command again"
            )))
        }
    }

    /// Starts a new session.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the session cannot start, and
    /// [`ClientError::Timeout`] if it is not ready in time.
    pub async fn create_session(&mut self, session: NewSession) -> Result<Session, ClientError> {
        self.open_session(session).await.map(|(session, _)| session)
    }

    /// Starts a new session, or gives the session that has the name of
    /// `session` if [`NewSession::reuse_existing`] is set. The second value
    /// is `false` if the session existed.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the session cannot start, if the
    /// name is not valid or in use, or if the daemon has no session names,
    /// and [`ClientError::Timeout`] if the session is not ready in time.
    pub async fn open_session(
        &mut self,
        session: NewSession,
    ) -> Result<(Session, bool), ClientError> {
        if session.name.is_some() {
            self.require(SESSION_NAMES, "session names")?;
        }
        let working_directory = match session.working_directory {
            Some(path) => path.into_os_string().into_string().map_err(|_| {
                ClientError::Failed("the working directory is not valid Unicode".to_owned())
            })?,
            None => String::new(),
        };
        let named = session.name.is_some();
        let request = SessionCreateRequest {
            command: session.command,
            working_directory,
            name: session.name.unwrap_or_default(),
            reuse_existing: session.reuse_existing,
            sandbox: String::new(),
        };
        match self.call(request::Body::SessionCreate(request)).await? {
            // A session without a name is always new.
            response::Result::SessionCreate(created) => created
                .session
                .map(|session| (Session::from(session), created.created || !named))
                .ok_or_else(|| ClientError::invalid_reply("the response has no session")),
            _ => Err(unexpected_result()),
        }
    }

    /// Gives the session `id` the name `name`, or removes its name for
    /// `None`. Returns the session with its new name.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier
    /// or if the command of the session ended, and [`ClientError::Failed`]
    /// if the name is not valid or in use, or if the daemon has no session
    /// names.
    pub async fn rename_session(
        &mut self,
        id: &str,
        name: Option<&str>,
    ) -> Result<Session, ClientError> {
        self.require(SESSION_NAMES, "session names")?;
        let request = SessionRenameRequest {
            id: id.to_owned(),
            name: name.unwrap_or_default().to_owned(),
        };
        match self.call(request::Body::SessionRename(request)).await? {
            response::Result::SessionRename(renamed) => renamed
                .session
                .map(Session::from)
                .ok_or_else(|| ClientError::invalid_reply("the response has no session")),
            _ => Err(unexpected_result()),
        }
    }

    /// The session that `target` names: its identifier, its name, or a
    /// unique prefix of one of them. [`resolve_session`](crate::resolve_session)
    /// has the rule.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session matches, and
    /// [`ClientError::AmbiguousTarget`] if the target can be more than one
    /// session.
    pub async fn find_session(&mut self, target: &str) -> Result<Session, ClientError> {
        let sessions = self.list_sessions(true).await?;
        crate::resolve_session(sessions, target)
    }

    /// The session that a user means when the user names none:
    /// [`most_recent_session`](crate::most_recent_session) has the rule.
    /// `None` if the host has no session that is not ended.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn most_recent_session(&mut self) -> Result<Option<Session>, ClientError> {
        let sessions = self.list_sessions(false).await?;
        Ok(crate::most_recent_session(sessions))
    }

    /// Lists the sessions of the host, in the sequence of their identifiers:
    /// the sessions whose command runs, the dormant sessions and the
    /// damaged sessions. With `include_ended`, also the sessions whose
    /// command ended and that the daemon still keeps.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn list_sessions(
        &mut self,
        include_ended: bool,
    ) -> Result<Vec<Session>, ClientError> {
        let request = request::Body::SessionList(SessionListRequest { include_ended });
        match self.call(request).await? {
            response::Result::SessionList(list) => {
                Ok(list.sessions.into_iter().map(Session::from).collect())
            }
            _ => Err(unexpected_result()),
        }
    }

    /// Starts the command of the dormant session `id` again, with the
    /// command, the working directory and the window size that it had. A
    /// session whose command runs stays as it is.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier
    /// or if the command of the session ended, and [`ClientError::Failed`]
    /// if the command cannot start.
    pub async fn start_session(&mut self, id: &str) -> Result<Session, ClientError> {
        let request = SessionStartRequest { id: id.to_owned() };
        match self.call(request::Body::SessionStart(request)).await? {
            response::Result::SessionStart(started) => started
                .session
                .map(Session::from)
                .ok_or_else(|| ClientError::invalid_reply("the response has no session")),
            _ => Err(unexpected_result()),
        }
    }

    /// Sets when the daemon starts the command of the session `id` again
    /// after the session became dormant.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier
    /// or if the command of the session ended.
    pub async fn set_restore_policy(
        &mut self,
        id: &str,
        policy: RestorePolicy,
    ) -> Result<(), ClientError> {
        let request = SessionSetRestorePolicyRequest {
            id: id.to_owned(),
            policy: wire_restore_policy(policy).into(),
        };
        match self
            .call(request::Body::SessionSetRestorePolicy(request))
            .await?
        {
            response::Result::SessionSetRestorePolicy(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    /// Reads output bytes of the session `id` that are before a position:
    /// one part of its scrollback. The session can have each state, also
    /// dormant and ended.
    ///
    /// The bytes are the last ones before `range.before` that the daemon
    /// has without a gap. To read the bytes before them, ask again with
    /// `before` equal to the `seq` of the result. No bytes: the daemon has
    /// no output of that epoch before the position.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier.
    pub async fn capture(
        &mut self,
        id: &str,
        range: CaptureRange,
    ) -> Result<CapturedOutput, ClientError> {
        let request = SessionCaptureRequest {
            id: id.to_owned(),
            epoch: range.epoch.unwrap_or(0),
            before_seq: range.before,
            max_bytes: range.max_bytes.unwrap_or(0),
        };
        match self.call(request::Body::SessionCapture(request)).await? {
            response::Result::SessionCapture(captured) => Ok(CapturedOutput {
                epoch: captured.epoch,
                seq: captured.seq,
                bytes: captured.data,
            }),
            _ => Err(unexpected_result()),
        }
    }

    /// Ends the session `id` and waits until its command has ended. A
    /// dormant or a damaged session leaves the host with its output.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier,
    /// and [`ClientError::Timeout`] if the command does not end in time.
    pub async fn kill_session(&mut self, id: &str) -> Result<(), ClientError> {
        let request = SessionKillRequest { id: id.to_owned() };
        match self.call(request::Body::SessionKill(request)).await? {
            response::Result::SessionKill(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    /// Attaches to the terminal of the session `id`. The connection becomes
    /// the terminal of the attach: the result is the screen and the output
    /// of the session, and the way to send input.
    ///
    /// An attach to a dormant session starts its command again, if the
    /// restore policy of the session is not [`RestorePolicy::Manual`].
    ///
    /// To send a request during the attach, for example
    /// [`resize_session`](Self::resize_session), use a second connection.
    ///
    /// This attach ends with an error when its connection ends.
    /// [`Host::attach`](crate::Host::attach) gives an attach that connects
    /// again.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier,
    /// and [`ClientError::Failed`] if the window size is not valid, if the
    /// daemon cannot read the terminal of the session, or if the session is
    /// dormant and cannot start.
    pub async fn attach(
        self,
        id: &str,
        options: AttachOptions,
    ) -> Result<(TerminalOutput, TerminalInput), ClientError> {
        let attached = self.attach_frames(id, options).await?;
        Ok((
            TerminalOutput::new(attached.output, attached.handle, options.resume),
            TerminalInput::new(attached.input, attached.handle),
        ))
    }

    /// Sends the attach request and changes the connection to the terminal
    /// channels.
    pub(crate) async fn attach_frames(
        mut self,
        id: &str,
        options: AttachOptions,
    ) -> Result<AttachedFrames, ClientError> {
        let size = options.size.unwrap_or(WindowSize {
            rows: 0,
            columns: 0,
        });
        let request = SessionAttachRequest {
            id: id.to_owned(),
            rows: size.rows.into(),
            columns: size.columns.into(),
            epoch: options.resume.map_or(0, |position| position.epoch),
            from_seq: options.resume.map_or(0, |position| position.seq),
        };
        let response::Result::SessionAttach(attached) =
            self.call(request::Body::SessionAttach(request)).await?
        else {
            return Err(unexpected_result());
        };
        let (output, input) = self.stream.into_frames();
        Ok(AttachedFrames {
            output,
            input,
            handle: attached.handle,
        })
    }

    /// Detaches each client that is attached to the session `id`. The
    /// session continues. Returns the number of clients.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier.
    pub async fn detach_session(&mut self, id: &str) -> Result<u32, ClientError> {
        let request = SessionDetachRequest { id: id.to_owned() };
        match self.call(request::Body::SessionDetach(request)).await? {
            response::Result::SessionDetach(detached) => Ok(detached.detached),
            _ => Err(unexpected_result()),
        }
    }

    /// Changes the window size of the session `id`. The session has the
    /// size of the client that attached or changed its size last.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier,
    /// and [`ClientError::Failed`] if the window size is not valid.
    pub async fn resize_session(&mut self, id: &str, size: WindowSize) -> Result<(), ClientError> {
        let request = SessionResizeRequest {
            id: id.to_owned(),
            rows: size.rows.into(),
            columns: size.columns.into(),
        };
        match self.call(request::Body::SessionResize(request)).await? {
            response::Result::SessionResize(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    /// Writes bytes to the terminal of the session `id`, as if the user
    /// typed them, and waits until the session has them. A client that is
    /// attached uses [`TerminalInput::send`], which does not wait.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier,
    /// and [`ClientError::Failed`] if the command of the session has ended.
    pub async fn send_input(&mut self, id: &str, bytes: &[u8]) -> Result<(), ClientError> {
        let request = SessionInputRequest {
            id: id.to_owned(),
            data: bytes.to_vec(),
        };
        match self.call(request::Body::SessionInput(request)).await? {
            response::Result::SessionInput(_) => Ok(()),
            _ => Err(unexpected_result()),
        }
    }

    /// Asks the daemon for its state.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn host_info(&mut self) -> Result<HostInfo, ClientError> {
        let request = request::Body::HostInfo(HostInfoRequest {});
        match self.call(request).await? {
            response::Result::HostInfo(info) => Ok(info.into()),
            _ => Err(unexpected_result()),
        }
    }

    /// Stops the daemon and waits until it has closed the connection. No
    /// session ends.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Timeout`] if the daemon does not close the
    /// connection in time.
    pub async fn stop_daemon(mut self) -> Result<(), ClientError> {
        let request = request::Body::HostStop(HostStopRequest {});
        let response::Result::HostStop(_) = self.call(request).await? else {
            return Err(unexpected_result());
        };
        // The daemon removes its socket before it closes the connections.
        let closed = async { while let Ok(Some(_)) = self.stream.receive().await {} };
        timeout(REQUEST_TIMEOUT, closed)
            .await
            .map_err(|_| request_timeout())
    }

    /// The connection, for a request after which it carries no more
    /// responses.
    pub(crate) fn into_stream(self) -> EnvelopeStream {
        self.stream
    }

    /// Sends one request and waits for its result. A failure result of the
    /// daemon becomes an error.
    pub(crate) async fn call(
        &mut self,
        body: request::Body,
    ) -> Result<response::Result, ClientError> {
        timeout(REQUEST_TIMEOUT, self.exchange(body))
            .await
            .map_err(|_| request_timeout())?
    }

    async fn exchange(&mut self, body: request::Body) -> Result<response::Result, ClientError> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        let request = Envelope {
            kind: Some(envelope::Kind::Request(Request {
                id,
                body: Some(body),
            })),
        };
        self.stream
            .send(&request)
            .await
            .map_err(ClientError::Connection)?;
        loop {
            let envelope = self
                .stream
                .receive()
                .await
                .map_err(ClientError::Connection)?
                .ok_or_else(|| {
                    ClientError::Connection(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the daemon closed the connection",
                    ))
                })?;
            // A connection with requests has no subscription, and it has one
            // request at a time. The client ignores each other envelope.
            let Some(envelope::Kind::Response(response)) = envelope.kind else {
                continue;
            };
            if response.id != id {
                continue;
            }
            return match response.result {
                Some(response::Result::Error(error)) => Err(error.into()),
                Some(result) => Ok(result),
                None => Err(ClientError::invalid_reply("the response has no result")),
            };
        }
    }
}

/// The terminal channels of an attach that the daemon accepted.
#[derive(Debug)]
pub(crate) struct AttachedFrames {
    pub(crate) output: FrameReader,
    pub(crate) input: FrameWriter,
    /// The session handle of the frames.
    pub(crate) handle: u32,
}

pub(crate) fn unexpected_result() -> ClientError {
    ClientError::invalid_reply("the response has the result of a different request")
}

fn request_timeout() -> ClientError {
    ClientError::Timeout(format!(
        "the daemon did not answer in {} seconds",
        REQUEST_TIMEOUT.as_secs()
    ))
}
