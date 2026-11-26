//! The control socket of the holder: accepts the connections of the daemon,
//! keeps the single-writer lease and answers the requests.

use std::fs::{self, Permissions};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use nomux_core::runtime_directory::check_socket_path;
use nomux_holder_protocol::wire::{ChildStatus, ErrorCode, Frame, Hello, Reply, Request};

use super::connection::{Answer, Connection, PendingWait, PendingWaits, send_answer};
use super::peer::peer_user_id;
use super::{journal, program_reports, redraw};
use crate::session::Session;

/// The largest number of connections at one time. During a daemon update,
/// the old daemon and the new daemon are connected together.
pub(crate) const MAX_CONNECTIONS: usize = 4;

const OWNER_READ_WRITE: u32 = 0o600;

pub(crate) struct ControlServer {
    listener: UnixListener,
    connections: [Option<Connection>; MAX_CONNECTIONS],
    /// The slot of the connection that has the single-writer lease.
    lease: Option<usize>,
    /// The read-only descriptor of the ring buffer. Each connection gets a
    /// copy with the hello.
    ring: OwnedFd,
}

impl ControlServer {
    /// Makes the socket at `path`. The directory of `path` must be private
    /// to the user.
    pub(crate) fn bind(path: &Path, ring: OwnedFd) -> io::Result<Self> {
        check_socket_path(path)?;
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, Permissions::from_mode(OWNER_READ_WRITE))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            connections: [const { None }; MAX_CONNECTIONS],
            lease: None,
            ring,
        })
    }

    pub(crate) fn listener(&self) -> BorrowedFd<'_> {
        self.listener.as_fd()
    }

    pub(crate) fn connection(&self, slot: usize) -> Option<BorrowedFd<'_>> {
        let connection = self.connections[slot].as_ref()?;
        Some(connection.stream.as_fd())
    }

    /// Accepts the connections that wait. A connection from a different
    /// user, and a connection when all slots are in use, is closed
    /// immediately.
    pub(crate) fn accept(&mut self, hello: &Hello) -> io::Result<()> {
        let mut hello_frame = Vec::new();
        hello.encode(&mut hello_frame);
        loop {
            let stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if is_failed_connection(&error) => continue,
                Err(error) => return Err(error),
            };
            if let Err(error) = self.admit(stream, &hello_frame) {
                eprintln!("nomux-holder: connection refused: {error}");
            }
        }
    }

    fn admit(&mut self, stream: UnixStream, hello_frame: &[u8]) -> io::Result<()> {
        if peer_user_id(&stream)? != rustix::process::getuid().as_raw() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let free_slot = self.connections.iter_mut().find(|slot| slot.is_none());
        let Some(slot) = free_slot else {
            return Err(io::Error::other("too many connections"));
        };
        *slot = Some(Connection::open(stream, hello_frame, self.ring.as_fd())?);
        Ok(())
    }

    /// Reads and answers the requests of one connection. Returns `true` if
    /// the connection got the exit status of the child.
    pub(crate) fn serve(&mut self, slot: usize, session: &mut Session) -> bool {
        let Some(connection) = self.connections[slot].as_mut() else {
            return false;
        };
        match serve_connection(connection, slot, &mut self.lease, session) {
            Ok(exit_status_delivered) => exit_status_delivered,
            Err(error) => {
                if error.kind() != io::ErrorKind::UnexpectedEof {
                    eprintln!("nomux-holder: connection closed: {error}");
                }
                self.close(slot);
                false
            }
        }
    }

    /// Answers the wait requests that can end now.
    pub(crate) fn complete_waits(&mut self, session: &Session) {
        let child_exited = session.child_exited();
        let write_position = session.write_position();
        let revision = session.program_reports_revision();
        for slot in 0..MAX_CONNECTIONS {
            let Some(connection) = self.connections[slot].as_mut() else {
                continue;
            };
            let Connection {
                stream,
                outgoing,
                waits,
                ..
            } = connection;
            let ready = [
                take_ready(&mut waits.output, child_exited, write_position)
                    .map(|request_id| (request_id, output(write_position))),
                take_ready(&mut waits.program_reports, child_exited, revision)
                    .map(|request_id| (request_id, program_reports_changed(revision))),
            ];
            let sent = ready
                .into_iter()
                .flatten()
                .try_for_each(|(request_id, reply)| {
                    send_answer(stream, outgoing, request_id, &reply.into())
                });
            if sent.is_err() {
                self.close(slot);
            }
        }
    }

    fn close(&mut self, slot: usize) {
        self.connections[slot] = None;
        if self.lease == Some(slot) {
            self.lease = None;
        }
    }
}

fn serve_connection(
    connection: &mut Connection,
    slot: usize,
    lease: &mut Option<usize>,
    session: &mut Session,
) -> io::Result<bool> {
    connection.receive()?;
    let Connection {
        stream,
        frames,
        outgoing,
        waits,
    } = connection;
    let mut exit_status_delivered = false;
    loop {
        let frame = frames
            .next_frame()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let Some(frame) = frame else {
            return Ok(exit_status_delivered);
        };
        let answer = answer(frame, slot, lease, waits, session);
        if let Some(answer) = answer {
            send_answer(stream, outgoing, frame.request_id, &answer)?;
            exit_status_delivered |=
                matches!(answer.reply, Reply::Status(status) if status != ChildStatus::Running);
        }
    }
}

/// Does one request. Returns `None` if the reply comes later.
fn answer(
    frame: Frame<'_>,
    slot: usize,
    lease: &mut Option<usize>,
    waits: &mut PendingWaits,
    session: &mut Session,
) -> Option<Answer> {
    let has_lease = *lease == Some(slot);
    let child_exited = session.child_exited();
    let pending = |after| PendingWait {
        request_id: frame.request_id,
        after,
    };
    let reply = match Request::decode(frame) {
        Err(error) => Reply::Error(error.into()),
        Ok(Request::AcquireLease) => {
            *lease = Some(slot);
            Reply::Done
        }
        Ok(Request::WaitOutput { after }) => {
            let position = session.write_position();
            let slot = &mut waits.output;
            return wait(slot, pending(after), position, child_exited, output).map(Answer::from);
        }
        Ok(Request::WaitProgramReports { after }) => {
            let revision = session.program_reports_revision();
            let slot = &mut waits.program_reports;
            return wait(
                slot,
                pending(after),
                revision,
                child_exited,
                program_reports_changed,
            )
            .map(Answer::from);
        }
        Ok(Request::Redraw) => return Some(redraw::answer(session)),
        Ok(Request::ProgramReports) => return Some(program_reports::answer(session)),
        Ok(Request::JournalRead { from, max_length }) => {
            return Some(journal::answer(session, from, max_length));
        }
        Ok(Request::Status) => Reply::Status(session.status()),
        Ok(Request::SessionToken) => Reply::SessionToken(session.token()),
        Ok(Request::Input(bytes)) => write_request(has_lease, || session.write_input(bytes)),
        Ok(Request::Resize(size)) => write_request(has_lease, || session.resize(size)),
        Ok(Request::Signal(signal)) => write_request(has_lease, || session.signal_child(signal)),
        Ok(Request::AssignSession {
            size,
            working_directory,
        }) => write_request(has_lease, || session.assign(size, working_directory)),
        Ok(Request::Checkpoint) => write_request(has_lease, || session.checkpoint()),
        // `Request` can get a variant that this holder does not answer yet.
        Ok(_) => Reply::Error(ErrorCode::UnknownKind),
    };
    Some(reply.into())
}

/// Does a wait request for a value that only increases: a position or a
/// revision. `current` is the value now.
///
/// Answers with `reply` of the value now if that value is larger than the
/// value that the daemon has, or if the child ended. If not, keeps the
/// request in `slot` until that is true, and returns `None`.
fn wait(
    slot: &mut Option<PendingWait>,
    request: PendingWait,
    current: u64,
    child_exited: bool,
    reply: fn(u64) -> Reply,
) -> Option<Reply> {
    if slot.is_some() {
        return Some(Reply::Error(ErrorCode::WaitPending));
    }
    if child_exited || current > request.after {
        return Some(reply(current));
    }
    *slot = Some(request);
    None
}

/// Takes the wait request out of `slot` if its reply can go now. Returns
/// its request identifier.
fn take_ready(slot: &mut Option<PendingWait>, child_exited: bool, current: u64) -> Option<u32> {
    slot.take_if(|wait| child_exited || current > wait.after)
        .map(|wait| wait.request_id)
}

fn output(write_position: u64) -> Reply {
    Reply::Output { write_position }
}

fn program_reports_changed(revision: u64) -> Reply {
    Reply::ProgramReportsChanged { revision }
}

/// Does a request that needs the single-writer lease.
fn write_request(has_lease: bool, operation: impl FnOnce() -> Result<(), ErrorCode>) -> Reply {
    if !has_lease {
        return Reply::Error(ErrorCode::LeaseRequired);
    }
    match operation() {
        Ok(()) => Reply::Done,
        Err(code) => Reply::Error(code),
    }
}

/// An error of `accept` that belongs to one connection, not to the listener.
fn is_failed_connection(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted
    )
}
