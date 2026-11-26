//! What the holder keeps of its session on disk: the session state
//! directory with the metadata, and the journal. A pooled holder has no
//! session yet, thus it has no files until its assignment.

use std::io;
use std::time::Instant;

use nomux_holder_protocol::wire::{ChildStatus, WindowSize};
use nomux_term::Terminal;

use crate::arguments::SessionArguments;
use crate::journal::{Journal, JournalLimits};
use crate::session_state_directory::SessionStateDirectory;
use crate::with_context;

pub(crate) struct SessionFiles {
    state_directory: SessionStateDirectory,
    journal: Journal,
}

impl SessionFiles {
    /// Makes the state directory of the session of `arguments` and starts
    /// its journal at the output position `start`. The first record of the
    /// journal is a checkpoint, before the next output.
    pub(crate) fn create(arguments: &SessionArguments, start: u64) -> io::Result<Self> {
        let state_directory = SessionStateDirectory::create(arguments)
            .map_err(|error| with_context("cannot make the session state directory", &error))?;
        let journal = Journal::create(
            state_directory.journal_directory(),
            state_directory.epoch(),
            start,
            JournalLimits::DEFAULT,
        )
        .map_err(|error| with_context("cannot start the journal", &error))?;
        Ok(Self {
            state_directory,
            journal,
        })
    }

    pub(crate) fn epoch(&self) -> u32 {
        self.state_directory.epoch()
    }

    pub(crate) fn journal(&mut self) -> &mut Journal {
        &mut self.journal
    }

    pub(crate) fn read_journal(&self, from: u64, max_length: usize) -> io::Result<(u64, Vec<u8>)> {
        self.journal.read(from, max_length)
    }

    pub(crate) fn flush_deadline(&self) -> Option<Instant> {
        self.journal.flush_deadline()
    }

    pub(crate) fn window_resized(&mut self, size: WindowSize) {
        self.journal.window_resized();
        report_state_error(self.state_directory.record_window_size(size));
    }

    /// Writes the last output and the end of the command.
    pub(crate) fn finish(&mut self, screen: &mut Terminal, size: WindowSize, status: ChildStatus) {
        self.journal.finish(screen, size);
        report_state_error(self.state_directory.record_end(status));
    }
}

/// The session continues when its metadata cannot be written.
fn report_state_error(result: io::Result<()>) {
    if let Err(error) = result {
        eprintln!("nomux-holder: cannot write the session metadata: {error}");
    }
}
