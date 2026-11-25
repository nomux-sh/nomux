//! One connection to a daemon.

use std::io;
use std::path::Path;
use std::time::Duration;

use nomux_proto::compatibility::{VersionRange, negotiate};
use nomux_proto::wire::v1::{
    self as wire, Envelope, Hello, HostInfoRequest, HostStopRequest, Request, SessionAttachRequest,
    SessionCreateRequest, SessionDetachRequest, SessionInputRequest, SessionKillRequest,
    SessionListRequest, SessionResizeRequest, envelope, request, response,
};
use nomux_transport::EnvelopeStream;
use tokio::time::timeout;

use crate::local_daemon;
use crate::terminal::{AttachOptions, TerminalInput, TerminalOutput};
use crate::{ClientError, ClientKind, HostInfo, NewSession, Session, WindowSize};

/// The longest time that a client waits for the response to one request.
/// The daemon ends each request of this protocol version earlier.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// A connection to a daemon on which the handshake is complete.
///
/// Each method sends one request and waits for its response.
#[derive(Debug)]
pub struct Client {
    stream: EnvelopeStream,
    next_request_id: u64,
}

impl Client {
    /// Connects to the daemon of this user on the local host.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::DaemonNotRunning`] if no daemon runs, and the
    /// errors of [`Client::connect`].
    pub async fn connect_local(kind: ClientKind) -> Result<Self, ClientError> {
        Self::connect(&local_daemon::socket_path()?, kind).await
    }

    /// Connects to the daemon of this user on the local host. If no daemon
    /// runs, starts one and waits until it answers.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::DaemonStart`] if the daemon cannot start,
    /// [`ClientError::Timeout`] if it does not answer in time, and the
    /// errors of [`Client::connect`].
    pub async fn connect_local_or_start(kind: ClientKind) -> Result<Self, ClientError> {
        let socket = local_daemon::socket_path()?;
        match Self::connect(&socket, kind).await {
            Err(ClientError::DaemonNotRunning) => local_daemon::start(&socket, kind).await,
            result => result,
        }
    }

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
        let mut client = Self {
            stream,
            next_request_id: 1,
        };
        client.handshake(kind).await?;
        Ok(client)
    }

    async fn handshake(&mut self, kind: ClientKind) -> Result<(), ClientError> {
        let local = VersionRange::CURRENT;
        let hello = Hello {
            version: local.version,
            oldest: local.oldest,
            capabilities: Vec::new(),
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
        Ok(())
    }

    /// Starts a new session.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Failed`] if the session cannot start, and
    /// [`ClientError::Timeout`] if it is not ready in time.
    pub async fn create_session(&mut self, session: NewSession) -> Result<Session, ClientError> {
        let working_directory = match session.working_directory {
            Some(path) => path.into_os_string().into_string().map_err(|_| {
                ClientError::Failed("the working directory is not valid Unicode".to_owned())
            })?,
            None => String::new(),
        };
        let request = SessionCreateRequest {
            command: session.command,
            working_directory,
        };
        match self.call(request::Body::SessionCreate(request)).await? {
            response::Result::SessionCreate(created) => created
                .session
                .map(Session::from)
                .ok_or_else(|| ClientError::invalid_reply("the response has no session")),
            _ => Err(unexpected_result()),
        }
    }

    /// Lists the sessions of the host, in the sequence of their identifiers.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn list_sessions(&mut self) -> Result<Vec<Session>, ClientError> {
        let request = request::Body::SessionList(SessionListRequest {});
        match self.call(request).await? {
            response::Result::SessionList(list) => {
                Ok(list.sessions.into_iter().map(Session::from).collect())
            }
            _ => Err(unexpected_result()),
        }
    }

    /// Ends the session `id` and waits until its command has ended.
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
    /// To send a request during the attach, for example
    /// [`resize_session`](Self::resize_session), use a second connection.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::NotFound`] if no session has the identifier,
    /// and [`ClientError::Failed`] if the window size is not valid or the
    /// daemon cannot read the terminal of the session.
    pub async fn attach(
        mut self,
        id: &str,
        options: AttachOptions,
    ) -> Result<(TerminalOutput, TerminalInput), ClientError> {
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
        Ok((
            TerminalOutput::new(output, attached.handle, attached.epoch, options.resume),
            TerminalInput::new(input, attached.handle),
        ))
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

    /// Sends one request and waits for its result. A failure result of the
    /// daemon becomes an error.
    async fn call(&mut self, body: request::Body) -> Result<response::Result, ClientError> {
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
            // This client subscribes to no events and has one request at a
            // time. It ignores each other envelope.
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

fn unexpected_result() -> ClientError {
    ClientError::invalid_reply("the response has the result of a different request")
}

fn request_timeout() -> ClientError {
    ClientError::Timeout(format!(
        "the daemon did not answer in {} seconds",
        REQUEST_TIMEOUT.as_secs()
    ))
}
