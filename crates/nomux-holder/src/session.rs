//! The session: the terminal, the child command, the ring buffer, the
//! screen state, the program reports and the journal.
//!
//! A pooled holder runs the command before its session exists. It has no
//! files until the daemon assigns the session to it.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::Instant;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::ring::RingWriter;
use nomux_holder_protocol::sandbox::FeatureSet;
use nomux_holder_protocol::wire::{
    ChildSignal, ChildStatus, ErrorCode, Hello, POOLED_EPOCH, ProtocolVersion, SessionToken,
    WindowSize,
};
use nomux_term::osc::OscRecord;
use nomux_term::osc::program_status::QUERY_REPLY;
use nomux_term::{Size, Terminal};
use rustix::event::PollFlags;
use rustix::io::Errno;

use crate::arguments::SessionArguments;
use crate::child::ChildProcess;
use crate::directory_fifo::DirectoryFifo;
use crate::discovery_directory::DiscoveryDirectory;
use crate::foreground_program::ForegroundProgram;
use crate::input_queue::InputQueue;
use crate::program_reports::ProgramReportStore;
use crate::pty;
use crate::resize_verification::ResizeVerification;
use crate::sandbox_start::{CommandSandbox, SandboxEntry};
use crate::session_environment::{self, CommandEnvironment};
use crate::session_files::SessionFiles;

/// The largest number of bytes that one read moves from the terminal to the
/// ring buffer. The kernel gives 4096 bytes at most for one read of a
/// pseudoterminal, and a small chunk keeps more history readable.
const OUTPUT_CHUNK: usize = 4096;

/// After the child ended, the holder reads this number of chunks at most. A
/// process in the background can write to the terminal without an end.
const EXIT_DRAIN_CHUNKS: usize = 256;

pub(crate) struct Session {
    id: SessionId,
    /// The master side of the pseudoterminal. `None` after the child ended.
    terminal: Option<OwnedFd>,
    /// `false` after all processes closed the slave side.
    terminal_readable: bool,
    child: ChildProcess,
    ring: RingWriter,
    /// The state of the screen after each byte that the ring buffer got. It
    /// keeps no scrollback.
    screen: Terminal,
    /// What the programs reported in each byte that the ring buffer got.
    reports: ProgramReportStore,
    /// The metadata of the session and each byte that the ring buffer got
    /// since the session exists, on disk. `None` while the holder is pooled.
    files: Option<SessionFiles>,
    /// The description of the session that a pooled holder becomes. `None`
    /// after the assignment, and for a holder that started with its
    /// session.
    pooled: Option<SessionArguments>,
    /// The named pipe that gives the command its working directory at the
    /// assignment. `None` after the command read it.
    directory_fifo: Option<DirectoryFifo>,
    /// The discovery files. The directory is removed when the session is
    /// dropped.
    discovery: DiscoveryDirectory,
    input: InputQueue,
    size: WindowSize,
    resize_verification: ResizeVerification,
    /// The secret that the command of the session has in its environment.
    /// It is only in memory: no file has it.
    token: SessionToken,
    /// The sandbox features that apply to the command.
    sandbox_features: FeatureSet,
    landlock_abi: u8,
}

impl Session {
    /// Opens the pseudoterminal and starts the child command. Without
    /// `files`, the holder is pooled until [`assign`](Self::assign).
    pub(crate) fn start(
        arguments: &SessionArguments,
        sandbox: Option<CommandSandbox>,
        ring: RingWriter,
        files: Option<SessionFiles>,
        discovery: DiscoveryDirectory,
    ) -> io::Result<Self> {
        let screen_size = screen_size(arguments.size).ok_or(io::ErrorKind::InvalidInput)?;
        let pty = pty::open(arguments.size)?;
        let token = session_environment::token()?;
        let directory_fifo = arguments
            .directory_fifo
            .as_deref()
            .map(DirectoryFifo::open)
            .transpose()?;
        let mut environment = CommandEnvironment::of_session(
            &arguments.session_id,
            &arguments.runtime_directory,
            &token,
        );
        let (sandbox_features, landlock_abi) =
            sandbox.as_ref().map_or((FeatureSet::EMPTY, 0), |sandbox| {
                (sandbox.enforced, sandbox.landlock_abi)
            });
        let mut command_line = arguments.command_line();
        let stub;
        let mut limits = None;
        if let Some(sandbox) = sandbox {
            environment.sandboxed(&arguments.runtime_directory, &sandbox.plan);
            match sandbox.entry {
                SandboxEntry::Child(prepared) => limits = Some(prepared),
                SandboxEntry::Stub(plan) => {
                    stub = enter_sandbox_stub(plan)?;
                    let after_launcher = arguments.launcher.len();
                    command_line.splice(after_launcher..after_launcher, stub.iter());
                }
            }
        }
        let child = ChildProcess::spawn(
            &command_line,
            environment,
            arguments.working_directory.as_deref(),
            pty.slave,
            limits,
        )?;
        Ok(Self {
            id: arguments.session_id.clone(),
            terminal: Some(pty.master),
            terminal_readable: true,
            child,
            ring,
            screen: Terminal::new(screen_size),
            reports: ProgramReportStore::default(),
            pooled: files.is_none().then(|| arguments.clone()),
            directory_fifo,
            files,
            discovery,
            input: InputQueue::default(),
            size: arguments.size,
            resize_verification: ResizeVerification::default(),
            token,
            sandbox_features,
            landlock_abi,
        })
    }

    pub(crate) fn hello(&self) -> Hello {
        Hello {
            protocol: ProtocolVersion::CURRENT,
            session_id: self.id.clone(),
            child_pid: self.child.pid(),
            size: self.size,
            epoch: self
                .files
                .as_ref()
                .map_or(POOLED_EPOCH, SessionFiles::epoch),
            sandbox: self.sandbox_features,
            landlock_abi: self.landlock_abi,
        }
    }

    /// Tells the daemon that it can connect. Call it after the start.
    pub(crate) fn mark_ready(&self) -> io::Result<()> {
        self.discovery.mark_ready()
    }

    /// `true` while the holder has no session.
    pub(crate) fn is_pooled(&self) -> bool {
        self.pooled.is_some()
    }

    /// Makes the pooled holder the holder of its session: writes the state
    /// directory with `working_directory` and `size`, starts the journal at
    /// the current position with a checkpoint of the screen, gives the
    /// discovery directory the name of the session, and then writes
    /// `working_directory` to the named pipe of the directory if the holder
    /// has one.
    ///
    /// The command already runs, thus the holder does not change its
    /// working directory: a shell that the daemon started with its start
    /// file reads the directory from the named pipe before its first
    /// prompt.
    pub(crate) fn assign(
        &mut self,
        size: WindowSize,
        working_directory: &[u8],
    ) -> Result<(), ErrorCode> {
        if self.child.has_exited() {
            return Err(ErrorCode::ChildExited);
        }
        let directory_bytes = working_directory;
        let working_directory = PathBuf::from(OsStr::from_bytes(directory_bytes));
        let Some(mut arguments) = self.pooled.clone() else {
            return Err(ErrorCode::InvalidPayload);
        };
        if !working_directory.is_absolute() {
            return Err(ErrorCode::InvalidPayload);
        }
        // The journal starts at a position that the screen corresponds to.
        if self.screen.incomplete_tail() != 0 {
            return Err(ErrorCode::Failed);
        }
        if size != self.size {
            self.resize(size)?;
        }
        arguments.working_directory = Some(working_directory);
        arguments.size = size;
        let mut files =
            SessionFiles::create(&arguments, self.ring.position()).map_err(|error| {
                eprintln!("nomux-holder: the assignment failed: {error}");
                ErrorCode::Failed
            })?;
        // The checkpoint now: the journal has the screen of the assignment
        // also when no output follows.
        files.journal().before_output(&mut self.screen, self.size);
        self.discovery
            .assign(&arguments.runtime_directory, &arguments.session_id)
            .map_err(|error| {
                eprintln!("nomux-holder: cannot rename the discovery directory: {error}");
                ErrorCode::Failed
            })?;
        self.files = Some(files);
        self.pooled = None;
        if let Some(fifo) = &mut self.directory_fifo
            && let Err(error) = fifo.send(directory_bytes)
        {
            // The command stays in its directory, and the session works.
            eprintln!("nomux-holder: cannot give the command its directory: {error}");
            self.directory_fifo = None;
        }
        Ok(())
    }

    /// The token of the session, for the daemon.
    pub(crate) fn token(&self) -> SessionToken {
        self.token
    }

    /// The terminal and the events that the event loop must wait for.
    pub(crate) fn terminal_interest(&self) -> Option<(BorrowedFd<'_>, PollFlags)> {
        let terminal = self.terminal.as_ref()?;
        let mut interest = PollFlags::empty();
        if self.terminal_readable {
            interest |= PollFlags::IN;
        }
        if !self.input.is_empty() {
            interest |= PollFlags::OUT;
        }
        (!interest.is_empty()).then_some((terminal.as_fd(), interest))
    }

    /// Moves one chunk of output from the terminal into the ring buffer,
    /// gives the same bytes to the screen state and to the journal, and
    /// keeps what the programs reported in them. Returns `false` if the
    /// terminal had no bytes.
    pub(crate) fn read_output(&mut self) -> io::Result<bool> {
        let Some(terminal) = &self.terminal else {
            return Ok(false);
        };
        let mut journal = self.files.as_mut().map(SessionFiles::journal);
        if let Some(journal) = &mut journal {
            journal.before_output(&mut self.screen, self.size);
        }
        let (screen, reports, input) = (&mut self.screen, &mut self.reports, &mut self.input);
        let foreground = ForegroundProgram::of(terminal.as_fd());
        let read = self.ring.write_with(OUTPUT_CHUNK, |chunk| {
            let count = rustix::io::read(terminal, &mut *chunk)?;
            screen.feed(&chunk[..count], |osc| {
                if let Some(record) = osc.record() {
                    keep_record(record, reports, input, &foreground);
                }
            });
            if let Some(journal) = &mut journal {
                journal.append(&chunk[..count]);
            }
            Ok::<_, Errno>(count)
        });
        match read {
            // No process has the slave side open: Linux reports `EIO`, macOS
            // reports the end of the file.
            Ok(0) | Err(Errno::IO) => {
                self.terminal_readable = false;
                Ok(false)
            }
            Ok(_) => {
                if let Some(journal) = journal {
                    journal.output_written(&mut self.screen, self.size);
                }
                // The shell writes its prompt after it read the directory.
                if self
                    .directory_fifo
                    .as_ref()
                    .is_some_and(DirectoryFifo::is_read)
                {
                    self.directory_fifo = None;
                }
                self.resize_verification
                    .output_read(terminal.as_fd(), self.size);
                self.answer_queries();
                self.flush_input();
                Ok(true)
            }
            Err(Errno::AGAIN | Errno::INTR) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// Gives the program the answers of the screen state to the queries in
    /// its output. The holder answers each query, also while clients are
    /// attached: a client does not give the queries to its terminal.
    fn answer_queries(&mut self) {
        let replies = self.screen.take_replies();
        // A program that sends queries and does not read the answers must
        // not make the input queue grow without a limit.
        if !replies.is_empty() && !self.input.is_backlogged() {
            self.input.push(&replies);
        }
    }

    pub(crate) fn write_position(&self) -> u64 {
        self.ring.position()
    }

    /// The position of the ring buffer that the screen corresponds to: the
    /// write position, or the start of the escape sequence or the UTF-8
    /// character that the output ends in. The bytes of a sequence that is
    /// not complete did not change the screen.
    pub(crate) fn redraw_position(&self) -> u64 {
        let incomplete_tail = self.screen.incomplete_tail() as u64;
        self.ring.position().saturating_sub(incomplete_tail)
    }

    /// The byte sequence that makes the current screen in a new terminal.
    /// The screen corresponds to [`redraw_position`](Self::redraw_position).
    pub(crate) fn redraw_sequence(&mut self) -> Vec<u8> {
        let mut sequence = Vec::new();
        self.screen.write_redraw(&mut sequence);
        sequence
    }

    /// The time at which the event loop must call
    /// [`flush_journal`](Self::flush_journal).
    pub(crate) fn journal_flush_deadline(&self) -> Option<Instant> {
        self.files.as_ref().and_then(SessionFiles::flush_deadline)
    }

    /// Writes the output bytes that wait in memory to the journal.
    pub(crate) fn flush_journal(&mut self) {
        if let Some(files) = &mut self.files {
            files.journal().flush(&mut self.screen, self.size);
        }
    }

    /// Writes a checkpoint of the current screen to the journal: see
    /// `Journal::checkpoint_now`. A pooled holder has no journal.
    pub(crate) fn checkpoint(&mut self) -> Result<(), ErrorCode> {
        let files = self.files.as_mut().ok_or(ErrorCode::Failed)?;
        files.journal().checkpoint_now(&mut self.screen, self.size);
        Ok(())
    }

    /// Output bytes from the journal: see `Journal::read`. A pooled holder
    /// has no journal: its end is the write position.
    pub(crate) fn read_journal(&self, from: u64, max_length: usize) -> io::Result<(u64, Vec<u8>)> {
        match &self.files {
            Some(files) => files.read_journal(from, max_length),
            None => Ok((self.ring.position(), Vec::new())),
        }
    }

    /// The number of changes to the program reports.
    pub(crate) fn program_reports_revision(&self) -> u64 {
        self.reports.revision()
    }

    pub(crate) fn encoded_program_reports(&self) -> Vec<u8> {
        self.reports.encode()
    }

    pub(crate) fn input_backlogged(&self) -> bool {
        self.input.is_backlogged()
    }

    pub(crate) fn write_input(&mut self, bytes: &[u8]) -> Result<(), ErrorCode> {
        if self.terminal.is_none() {
            return Err(ErrorCode::ChildExited);
        }
        self.input.push(bytes);
        self.reports.input_written();
        self.flush_input();
        Ok(())
    }

    pub(crate) fn flush_input(&mut self) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        if self.input.flush(terminal).is_err() {
            // The terminal has no reader. The input has no destination.
            self.input.clear();
        }
    }

    pub(crate) fn resize(&mut self, size: WindowSize) -> Result<(), ErrorCode> {
        let terminal = self.terminal.as_ref().ok_or(ErrorCode::ChildExited)?;
        let screen_size = screen_size(size).ok_or(ErrorCode::InvalidPayload)?;
        pty::resize(terminal, size).map_err(|_| ErrorCode::Failed)?;
        self.screen.resize(screen_size);
        self.size = size;
        self.resize_verification.window_resized();
        if let Some(files) = &mut self.files {
            files.window_resized(size);
        }
        Ok(())
    }

    pub(crate) fn signal_child(&self, signal: ChildSignal) -> Result<(), ErrorCode> {
        if self.child.has_exited() {
            return Err(ErrorCode::ChildExited);
        }
        self.child.signal(signal).map_err(|_| ErrorCode::Failed)
    }

    pub(crate) fn status(&self) -> ChildStatus {
        self.child.status()
    }

    pub(crate) fn child_exited(&self) -> bool {
        self.child.has_exited()
    }

    /// Collects the exit status if the child ended. On the call that finds
    /// the end, reads the output that the child left in the terminal, closes
    /// the terminal and returns `true`.
    pub(crate) fn reap_child(&mut self) -> io::Result<bool> {
        if !self.child.reap()? {
            return Ok(false);
        }
        for _ in 0..EXIT_DRAIN_CHUNKS {
            if !self.read_output()? {
                break;
            }
        }
        self.terminal = None;
        self.input.clear();
        if let Some(files) = &mut self.files {
            files.finish(&mut self.screen, self.size, self.child.status());
        }
        Ok(true)
    }
}

/// Keeps what a program reported with one OSC sequence, and answers the
/// program status query.
fn keep_record(
    record: OscRecord,
    reports: &mut ProgramReportStore,
    input: &mut InputQueue,
    foreground: &ForegroundProgram<'_>,
) {
    match record {
        // A program that sends queries and does not read the answers must
        // not make the input queue grow without a limit.
        OscRecord::ProgramStatusQuery if input.is_backlogged() => {}
        OscRecord::ProgramStatusQuery => input.push(QUERY_REPLY),
        OscRecord::ProgramStatus(report) => {
            reports.report_status(report, |app| foreground.agrees_with(app));
        }
        OscRecord::PromptMark(mark) => reports.mark_prompt(mark),
        OscRecord::Notification(notification) => reports.notify(notification),
    }
}

/// The size of the screen state for a window. `None` if the window has more
/// cells than the holder keeps in memory, or if the screen state cannot have
/// the size: no row, or fewer than [`Size::MIN_COLUMNS`] columns.
/// The words that make a launcher start this binary as the sandbox stub,
/// before the command: `nomux-holder --enter-sandbox <plan> --`.
fn enter_sandbox_stub(plan: PathBuf) -> io::Result<[OsString; 4]> {
    Ok([
        std::env::current_exe()?.into_os_string(),
        "--enter-sandbox".into(),
        plan.into_os_string(),
        "--".into(),
    ])
}

fn screen_size(size: WindowSize) -> Option<Size> {
    if size.cells() > WindowSize::MAX_CELLS {
        return None;
    }
    Size::new(size.rows, size.columns)
}
