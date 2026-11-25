//! The one loop of the holder. It waits in `poll` with no timeout while the
//! child runs, thus an idle holder uses no CPU time.

use std::array;
use std::io;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use crate::child_exit_signal::ChildExitSignal;
use crate::control::{ControlServer, MAX_CONNECTIONS};
use crate::session::Session;

const CHILD_EXIT: usize = 0;
const LISTENER: usize = 1;
const TERMINAL: usize = 2;
const FIRST_CONNECTION: usize = 3;
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
    loop {
        let ready = wait(&session, &server, &child_exit, exit_deadline)?;

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
        server.complete_waits(session.write_position(), session.child_exited());

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

fn timespec(duration: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: duration.subsec_nanos().into(),
    }
}
