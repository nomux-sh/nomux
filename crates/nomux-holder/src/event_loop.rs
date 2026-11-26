//! The one loop of the holder. While the child runs, it waits in `poll`
//! with no timeout, or until the journal must write the output bytes that
//! wait in memory. Thus an idle holder uses no CPU time.
//!
//! A pooled holder belongs to the daemon that started it: its standard
//! input is a pipe from that daemon. When the pipe closes, because the
//! daemon ended or does not need the holder, a holder that is still pooled
//! ends. Thus a pooled holder never stays without a daemon.

use std::array;
use std::io::{self, Stdin};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use nomux_holder_protocol::wire::ChildSignal;

use crate::child_exit_signal::ChildExitSignal;
use crate::control::{ControlServer, MAX_CONNECTIONS};
use crate::session::Session;

const CHILD_EXIT: usize = 0;
const LISTENER: usize = 1;
const TERMINAL: usize = 2;
const DAEMON_PIPE: usize = 3;
const FIRST_CONNECTION: usize = 4;
const DESCRIPTORS: usize = FIRST_CONNECTION + MAX_CONNECTIONS;

/// Runs the session until it ends.
///
/// After the child ends, the holder keeps the exit status until a connection
/// reads it with a status request, for `exit_timeout` at most.
pub(crate) fn run(
    mut session: Session,
    mut server: ControlServer,
    mut child_exit: ChildExitSignal,
    exit_timeout: Duration,
) -> io::Result<()> {
    let mut exit_deadline = None;
    let daemon_pipe = io::stdin();
    loop {
        let flush_deadline = session.journal_flush_deadline();
        let deadline = earliest(exit_deadline, flush_deadline);
        let ready = wait(&session, &server, &child_exit, &daemon_pipe, deadline)?;

        if session.is_pooled() && !ready[DAEMON_PIPE].is_empty() && is_closed(&daemon_pipe) {
            // The command of a holder without a session has no user.
            let _ = session.signal_child(ChildSignal::Hangup);
            return Ok(());
        }
        if !ready[CHILD_EXIT].is_empty() {
            child_exit.clear()?;
            if session.reap_child()? {
                exit_deadline = Some(Instant::now() + exit_timeout);
            }
        }
        if ready[TERMINAL].contains(PollFlags::OUT) {
            session.flush_input();
        }
        if !ready[TERMINAL].difference(PollFlags::OUT).is_empty() {
            session.read_output()?;
        }
        if !ready[LISTENER].is_empty() {
            server.accept(&session.hello())?;
        }
        let mut exit_status_delivered = false;
        for slot in 0..MAX_CONNECTIONS {
            if !ready[FIRST_CONNECTION + slot].is_empty() {
                exit_status_delivered |= server.serve(slot, &mut session);
            }
        }
        server.complete_waits(&session);
        if flush_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            session.flush_journal();
        }

        let ended = exit_deadline
            .is_some_and(|deadline| exit_status_delivered || Instant::now() >= deadline);
        if ended {
            return Ok(());
        }
    }
}

/// Waits until a descriptor is ready or the deadline passes. Returns the
/// events of each descriptor.
fn wait(
    session: &Session,
    server: &ControlServer,
    child_exit: &ChildExitSignal,
    daemon_pipe: &Stdin,
    deadline: Option<Instant>,
) -> io::Result<[PollFlags; DESCRIPTORS]> {
    // A position that is not in use holds the listener with no events.
    let unused = || PollFd::from_borrowed_fd(server.listener(), PollFlags::empty());
    let mut descriptors: [PollFd<'_>; DESCRIPTORS] = array::from_fn(|_| unused());
    descriptors[CHILD_EXIT] = PollFd::from_borrowed_fd(child_exit.descriptor(), PollFlags::IN);
    descriptors[LISTENER] = PollFd::from_borrowed_fd(server.listener(), PollFlags::IN);
    if let Some((terminal, interest)) = session.terminal_interest() {
        descriptors[TERMINAL] = PollFd::from_borrowed_fd(terminal, interest);
    }
    if session.is_pooled() {
        descriptors[DAEMON_PIPE] = PollFd::from_borrowed_fd(daemon_pipe.as_fd(), PollFlags::IN);
    }
    // While the terminal does not take the input, the requests wait in the
    // socket.
    if !session.input_backlogged() {
        for slot in 0..MAX_CONNECTIONS {
            if let Some(connection) = server.connection(slot) {
                descriptors[FIRST_CONNECTION + slot] =
                    PollFd::from_borrowed_fd(connection, PollFlags::IN);
            }
        }
    }

    loop {
        let timeout = deadline.map(|deadline| timespec(deadline - Instant::now()));
        match poll(&mut descriptors, timeout.as_ref()) {
            Ok(_) => return Ok(array::from_fn(|index| descriptors[index].revents())),
            Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// `true` if the pipe from the daemon has ended. `poll` reported it as
/// readable, thus the read does not wait. The daemon writes no byte.
fn is_closed(daemon_pipe: &Stdin) -> bool {
    let mut discarded = [0; 64];
    match rustix::io::read(daemon_pipe, &mut discarded) {
        Ok(read) => read == 0,
        Err(error) => !matches!(error, Errno::AGAIN | Errno::INTR),
    }
}

fn earliest(first: Option<Instant>, second: Option<Instant>) -> Option<Instant> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (deadline, None) | (None, deadline) => deadline,
    }
}

fn timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: duration.subsec_nanos().into(),
    }
}
