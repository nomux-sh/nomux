//! One SSH connection to a host: the TCP connection or the jump hosts, the
//! host key check, the authentication, and the channels to the Unix socket
//! of the daemon.

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use russh::client::{self, Handle, Msg};
use russh::keys::Algorithm;
use russh::{ChannelOpenFailure, ChannelStream, Preferred};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::authentication::authenticate;
use super::config::{self, HostSettings};
use super::host_key_check::HostKeyCheck;
use super::known_hosts::{KnownHosts, host_label};
use super::{SshEnvironment, SshError, SshTarget};
use crate::EnvelopeStream;

/// OpenSSH has no limit. A configuration with more jumps than this is a
/// loop.
const MAX_JUMP_HOSTS: usize = 8;
/// The receive window of a channel. The host sends at most this number of
/// bytes that the client did not take. A small window keeps the output of
/// a flood short behind the screen: the daemon then gives the client the
/// current screen and not each byte.
const CHANNEL_WINDOW: u32 = 256 * 1024;
const MAXIMUM_PACKET_SIZE: u32 = 32 * 1024;
/// The packets of one channel that wait for their reader.
const CHANNEL_BUFFER_PACKETS: usize = 8;
/// The address that a jump host gets as the origin of the connection.
const JUMP_ORIGIN: (&str, u32) = ("127.0.0.1", 0);

/// An SSH connection on which the user is authenticated.
///
/// nomux never asks for agent forwarding, for a terminal or for a port
/// forward on this connection.
pub struct SshConnection {
    session: Handle<HostKeyCheck>,
    description: String,
    host_key: String,
    /// The connection to the last jump host. This connection runs in a
    /// channel of it.
    _jump_host: Option<Box<SshConnection>>,
}

impl SshConnection {
    /// Connects to `target` with the configuration of the user
    /// (`~/.ssh/config`), checks the host key and authenticates the user.
    ///
    /// # Errors
    ///
    /// Returns the reason: the configuration, the network, the host key or
    /// the authentication. The same applies to each jump host.
    pub async fn connect(
        target: &SshTarget,
        environment: &SshEnvironment,
    ) -> Result<Self, SshError> {
        connect_through(target, None, environment, 0).await
    }

    /// `user@host port n`, for a message.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The type and the SHA-256 fingerprint of the host key.
    #[must_use]
    pub fn host_key(&self) -> &str {
        &self.host_key
    }

    /// Returns `true` if the connection ended.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.session.is_closed()
    }

    /// Opens a channel to the Unix socket `path` on the host
    /// (`direct-streamlocal@openssh.com`). The channel is one connection to
    /// the process that listens there, with its own flow control.
    ///
    /// # Errors
    ///
    /// Returns [`SshError::SocketUnavailable`] if nothing listens on the
    /// socket, [`SshError::ForwardingRefused`] if the SSH server does not
    /// permit the channel, and [`SshError::Disconnected`] if the connection
    /// ended.
    pub async fn open_unix_socket(&self, path: &str) -> Result<EnvelopeStream, SshError> {
        let channel = self.session.channel_open_direct_streamlocal(path).await?;
        let (read, write) = tokio::io::split(channel.into_stream());
        Ok(EnvelopeStream::new(read, EndOnDrop(Some(write))))
    }

    pub(super) fn session(&self) -> &Handle<HostKeyCheck> {
        &self.session
    }
}

impl fmt::Debug for SshConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SshConnection")
            .field("description", &self.description)
            .field("host_key", &self.host_key)
            .finish_non_exhaustive()
    }
}

type Connecting<'a> = Pin<Box<dyn Future<Output = Result<SshConnection, SshError>> + Send + 'a>>;

/// Connects to `target`. `jump_hosts` replaces the `ProxyJump` list of the
/// configuration of `target`: a host of a `ProxyJump` list is reached
/// through the hosts before it in the list.
fn connect_through<'a>(
    target: &'a SshTarget,
    jump_hosts: Option<&'a [SshTarget]>,
    environment: &'a SshEnvironment,
    depth: usize,
) -> Connecting<'a> {
    Box::pin(async move {
        if depth > MAX_JUMP_HOSTS {
            return Err(SshError::InvalidTarget {
                target: target.to_string(),
                problem: "the configuration has too many jump hosts",
            });
        }
        let settings = config::resolve(target, environment)?;
        let jump_hosts = jump_hosts.unwrap_or(&settings.proxy_jump);
        let host = host_label(&settings.host_key_alias, settings.port);
        let known_hosts = load_known_hosts(&settings)?;
        let configuration = Arc::new(client_configuration(&settings, &known_hosts, &host));
        let accepted = Arc::new(OnceLock::new());
        let check = HostKeyCheck {
            known_hosts,
            host,
            rule: settings.strict_host_key_checking,
            new_keys_file: settings.user_known_hosts_files.first().cloned(),
            target: target.clone(),
            accepted: Arc::clone(&accepted),
        };

        let start = async {
            let (mut session, jump_host) = match jump_hosts.split_last() {
                None => {
                    let stream = connect_tcp(&settings).await?;
                    let session = client::connect_stream(configuration, stream, check).await?;
                    (session, None)
                }
                Some((last, earlier)) => {
                    let jump_host =
                        connect_through(last, Some(earlier), environment, depth + 1).await?;
                    let stream = jump_host.open_tcp(&settings).await?;
                    let session = client::connect_stream(configuration, stream, check).await?;
                    (session, Some(Box::new(jump_host)))
                }
            };
            authenticate(&mut session, &settings, environment).await?;
            Ok::<_, SshError>((session, jump_host))
        };
        let (session, jump_host) =
            timeout(settings.connect_timeout, start)
                .await
                .map_err(|_| SshError::Connect {
                    host: settings.host_name.clone(),
                    port: settings.port,
                    source: io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "no connection in {} seconds",
                            settings.connect_timeout.as_secs()
                        ),
                    ),
                })??;

        Ok(SshConnection {
            session,
            description: format!(
                "{}@{} port {}",
                settings.user, settings.host_name, settings.port
            ),
            host_key: accepted.get().cloned().unwrap_or_default(),
            _jump_host: jump_host,
        })
    })
}

impl SshConnection {
    /// Opens a channel from this host to the host of `settings`: the TCP
    /// connection of the next SSH connection.
    async fn open_tcp(&self, settings: &HostSettings) -> Result<ChannelStream<Msg>, SshError> {
        let (origin, origin_port) = JUMP_ORIGIN;
        let channel = self
            .session
            .channel_open_direct_tcpip(
                settings.host_name.clone(),
                settings.port.into(),
                origin,
                origin_port,
            )
            .await;
        match channel {
            Ok(channel) => Ok(channel.into_stream()),
            Err(russh::Error::ChannelOpenFailure(reason)) => Err(SshError::Connect {
                host: settings.host_name.clone(),
                port: settings.port,
                source: io::Error::other(match reason {
                    ChannelOpenFailure::AdministrativelyProhibited => format!(
                        "the jump host {} does not permit TCP forwarding",
                        self.description
                    ),
                    _ => format!(
                        "the jump host {} cannot connect to the host",
                        self.description
                    ),
                }),
            }),
            Err(error) => Err(error.into()),
        }
    }
}

async fn connect_tcp(settings: &HostSettings) -> Result<TcpStream, SshError> {
    let connect_error = |source| SshError::Connect {
        host: settings.host_name.clone(),
        port: settings.port,
        source,
    };
    let stream = TcpStream::connect((settings.host_name.as_str(), settings.port))
        .await
        .map_err(connect_error)?;
    // A key and its echo are small packets. They must not wait for more
    // data.
    stream.set_nodelay(true).map_err(connect_error)?;
    Ok(stream)
}

fn load_known_hosts(settings: &HostSettings) -> Result<KnownHosts, SshError> {
    let files = [
        settings.user_known_hosts_files.as_slice(),
        settings.global_known_hosts_files.as_slice(),
    ]
    .concat();
    KnownHosts::load(&files).map_err(|error| SshError::Configuration {
        file: files.first().cloned().unwrap_or_default(),
        line: 0,
        problem: format!("a file of known hosts cannot be read: {error}"),
    })
}

fn client_configuration(
    settings: &HostSettings,
    known_hosts: &KnownHosts,
    host: &str,
) -> client::Config {
    // The key types that the files of known hosts have for the host come
    // first, as in OpenSSH.
    let known_types = known_hosts.key_types(host);
    let is_known = |algorithm: &Algorithm| {
        let key_type = match algorithm {
            Algorithm::Rsa { .. } => "ssh-rsa",
            other => other.as_str(),
        };
        known_types.contains(&key_type)
    };
    let (mut host_key_algorithms, others): (Vec<Algorithm>, Vec<Algorithm>) =
        Preferred::DEFAULT.key.iter().cloned().partition(is_known);
    host_key_algorithms.extend(others);

    client::Config {
        window_size: CHANNEL_WINDOW,
        maximum_packet_size: MAXIMUM_PACKET_SIZE,
        channel_buffer_size: CHANNEL_BUFFER_PACKETS,
        keepalive_interval: settings.server_alive_interval,
        keepalive_max: settings.server_alive_count_max,
        preferred: Preferred {
            key: host_key_algorithms.into(),
            ..Preferred::DEFAULT
        },
        ..client::Config::default()
    }
}

/// The sending direction of a channel. When the value is dropped, the host
/// gets the end of the direction, as the peer of a Unix socket does.
struct EndOnDrop(Option<tokio::io::WriteHalf<ChannelStream<Msg>>>);

impl AsyncWrite for EndOnDrop {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.0.as_mut() {
            Some(write) => Pin::new(write).poll_write(context, bytes),
            None => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.0.as_mut() {
            Some(write) => Pin::new(write).poll_flush(context),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.0.as_mut() {
            Some(write) => Pin::new(write).poll_shutdown(context),
            None => Poll::Ready(Ok(())),
        }
    }
}

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        let Some(mut write) = self.0.take() else {
            return;
        };
        // The end of a channel is a message, and a drop cannot wait. If the
        // runtime is gone, the connection is gone too.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = write.shutdown().await;
            });
        }
    }
}
