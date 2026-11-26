//! Runs `nomux` in a pseudoterminal, as a person does: the test types keys
//! and reads the screen that the output makes in a terminal.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use nomux_term::{Position, Size, Style, Terminal};
use rustix::fs::{Mode, OFlags};
use rustix::io::{FdFlags, fcntl_setfd};
use rustix::process::Signal;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{LocalModes, Winsize, tcgetattr, tcsetwinsize};

use crate::{TIMEOUT, raw_pid};

const READ_INTERVAL: Duration = Duration::from_millis(10);
/// The time in which the last output of a process that ended arrives.
const LAST_OUTPUT_TIME: Duration = Duration::from_millis(100);

/// A `nomux` process whose standard input, output and error are one
/// pseudoterminal, and the screen of that terminal.
///
/// The process is stopped when the value is dropped.
#[derive(Debug)]
pub struct TerminalClient {
    process: Child,
    /// The side of the terminal emulator.
    master: File,
    /// The side of the process. The test keeps it open: on macOS, a
    /// terminal that no process has open discards the output that the
    /// emulator did not read yet.
    terminal: File,
    /// The output of the process, from a thread that reads the terminal.
    output: Receiver<Vec<u8>>,
    reading_paused: Arc<AtomicBool>,
    /// The terminal state that the output made.
    screen: Terminal,
    /// The output of the process that the test read.
    raw_output: Vec<u8>,
    /// The number of bytes of output that the test did not keep.
    discarded: u64,
}

impl TerminalClient {
    /// Starts `command` in a new pseudoterminal with a window of `rows` and
    /// `columns`.
    ///
    /// # Panics
    ///
    /// Panics if the pseudoterminal or the process cannot start.
    #[must_use]
    pub fn start(mut command: Command, rows: u16, columns: u16) -> Self {
        let (master, terminal) = pseudoterminal(rows, columns);
        let standard = || Stdio::from(terminal.try_clone().expect("a descriptor can be copied"));
        let process = command
            .stdin(standard())
            .stdout(standard())
            .stderr(standard())
            .spawn()
            .expect("the CLI starts");

        let master = File::from(master);
        let reading_paused = Arc::new(AtomicBool::new(false));
        let output = read_in_thread(
            master.try_clone().expect("a descriptor can be copied"),
            Arc::clone(&reading_paused),
        );
        Self {
            process,
            master,
            terminal,
            output,
            reading_paused,
            screen: Terminal::new(size(rows, columns)),
            raw_output: Vec::new(),
            discarded: 0,
        }
    }

    /// The process ID of the `nomux` process.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.process.id()
    }

    /// Types `keys` on the keyboard of the terminal.
    ///
    /// # Panics
    ///
    /// Panics if the terminal is closed.
    pub fn type_keys(&mut self, keys: &[u8]) {
        self.master
            .write_all(keys)
            .expect("the terminal takes the keys");
    }

    /// The text of each row of the screen, without the spaces at its end.
    #[must_use]
    pub fn rows(&self) -> Vec<String> {
        let screen = self.screen.screen();
        let size = screen.size();
        (0..size.rows())
            .map(|row| {
                let text: String = (0..size.columns())
                    .filter_map(|column| screen.cell(Position { row, column }))
                    .map(|cell| cell.character)
                    .collect();
                text.trim_end().to_owned()
            })
            .collect()
    }

    /// The style of the cell at `row` and `column`.
    ///
    /// # Panics
    ///
    /// Panics if the position is not on the screen.
    #[must_use]
    pub fn style_at(&self, row: u16, column: u16) -> Style {
        let cell = self.screen.screen().cell(Position { row, column });
        cell.expect("the position is on the screen").style
    }

    /// Reads the output until the rows of the screen satisfy `condition`.
    /// Returns the rows.
    ///
    /// # Panics
    ///
    /// Panics after 20 seconds, and if the process ends first.
    pub fn wait_for_rows(
        &mut self,
        what: &str,
        condition: impl Fn(&[String]) -> bool,
    ) -> Vec<String> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let rows = self.rows();
            if condition(&rows) {
                return rows;
            }
            assert!(Instant::now() < deadline, "timeout: {what}: {rows:#?}");
            assert!(
                self.read_output(),
                "the process ended: {what}: {rows:#?}: {}",
                self.text_output()
            );
        }
    }

    /// Reads the output until a row of the screen contains `text`.
    ///
    /// # Panics
    ///
    /// Panics after 20 seconds, and if the process ends first.
    pub fn wait_for_text(&mut self, text: &str) -> Vec<String> {
        self.wait_for_rows(text, |rows| rows.iter().any(|row| row.contains(text)))
    }

    /// Reads the output that arrived until now and does not wait for more.
    pub fn read_pending_output(&mut self) {
        while let Ok(bytes) = self.output.try_recv() {
            self.keep(&bytes);
        }
    }

    /// Takes the output that arrived and does not keep it. Returns the
    /// number of bytes that the process wrote until now.
    pub fn discard_output(&mut self) -> u64 {
        while let Ok(bytes) = self.output.try_recv() {
            self.discarded += bytes.len() as u64;
        }
        self.discarded + self.raw_output.len() as u64
    }

    /// Gives the window of the terminal a new size and tells the process.
    ///
    /// # Panics
    ///
    /// Panics if the terminal or the process is gone.
    pub fn resize(&mut self, rows: u16, columns: u16) {
        tcsetwinsize(&self.terminal, window(rows, columns)).expect("the terminal takes the size");
        self.screen.resize(size(rows, columns));
        // The process has no controlling terminal, thus the terminal does
        // not send the signal itself.
        rustix::process::kill_process(raw_pid(self.pid()), Signal::WINCH)
            .expect("the process gets SIGWINCH");
    }

    /// Stops the read of the output: the terminal becomes a slow terminal
    /// that takes no more bytes.
    pub fn pause_reading(&self) {
        self.reading_paused.store(true, Ordering::Relaxed);
    }

    /// Reads the output again after [`pause_reading`](Self::pause_reading).
    pub fn resume_reading(&self) {
        self.reading_paused.store(false, Ordering::Relaxed);
    }

    /// The output of the process that the test read until now, as text.
    #[must_use]
    pub fn text_output(&self) -> String {
        String::from_utf8_lossy(&self.raw_output).into_owned()
    }

    /// `true` if the terminal is in raw mode: no echo and no line editing.
    ///
    /// # Panics
    ///
    /// Panics if the mode of the terminal cannot be read.
    #[must_use]
    pub fn is_in_raw_mode(&self) -> bool {
        let modes = tcgetattr(&self.terminal)
            .expect("the terminal has a mode")
            .local_modes;
        !modes.intersects(LocalModes::ICANON | LocalModes::ECHO)
    }

    /// Reads the output until the process ends. Returns the exit code.
    ///
    /// # Panics
    ///
    /// Panics if the process does not end in 20 seconds, or if a signal
    /// ended it.
    pub fn wait_for_exit(&mut self) -> i32 {
        let deadline = Instant::now() + TIMEOUT;
        while self.read_output() {
            assert!(
                Instant::now() < deadline,
                "the process did not end: {}",
                self.text_output()
            );
        }
        let status = self.process.wait().expect("the process can be awaited");
        status.code().expect("the process ended with a code")
    }

    /// Waits until the process writes more output.
    ///
    /// # Panics
    ///
    /// Panics after 20 seconds, and if the process ends first.
    pub fn wait_for_output(&mut self) {
        let seen = self.raw_output.len();
        let deadline = Instant::now() + TIMEOUT;
        while self.raw_output.len() == seen {
            assert!(Instant::now() < deadline, "timeout: more output");
            assert!(self.read_output(), "the process ended");
        }
    }

    /// Takes the output that arrived, or waits a short time for it. Returns
    /// `false` if the process ended and the test has all of its output.
    fn read_output(&mut self) -> bool {
        match self.output.recv_timeout(READ_INTERVAL) {
            Ok(bytes) => {
                self.keep(&bytes);
                true
            }
            Err(RecvTimeoutError::Disconnected) => false,
            Err(RecvTimeoutError::Timeout) => !self.read_last_output_after_exit(),
        }
    }

    /// If the process ended, reads the output that it wrote before its end
    /// and returns `true`.
    fn read_last_output_after_exit(&mut self) -> bool {
        let exited = self.process.try_wait().expect("the process can be awaited");
        if exited.is_none() {
            return false;
        }
        while let Ok(bytes) = self.output.recv_timeout(LAST_OUTPUT_TIME) {
            self.keep(&bytes);
        }
        true
    }
}

impl TerminalClient {
    /// Applies the output to the screen. The terminal answers the queries
    /// in the output, as the terminal of a user does.
    fn keep(&mut self, output: &[u8]) {
        self.screen.feed(output, |_osc| {});
        let replies = self.screen.take_replies();
        if !replies.is_empty() {
            self.type_keys(&replies);
        }
        self.raw_output.extend_from_slice(output);
    }
}

impl Drop for TerminalClient {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Opens a pseudoterminal. Returns the side of the terminal emulator and the
/// side of the process.
///
/// The two descriptors are close-on-exec. If a daemon that the process
/// starts kept one of them, the terminal would not close with the process.
fn pseudoterminal(rows: u16, columns: u16) -> (OwnedFd, File) {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).expect("a pseudoterminal opens");
    fcntl_setfd(&master, FdFlags::CLOEXEC).expect("the descriptor takes the flag");
    grantpt(&master).expect("the pseudoterminal can be used");
    unlockpt(&master).expect("the pseudoterminal can be used");
    let name = ptsname(&master, Vec::new()).expect("the pseudoterminal has a name");
    let flags = OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC;
    let terminal = rustix::fs::open(name, flags, Mode::empty()).expect("the pseudoterminal opens");
    tcsetwinsize(&terminal, window(rows, columns)).expect("the terminal takes the size");
    (master, File::from(terminal))
}

/// Reads the terminal in a thread, thus a test can wait for output with a
/// time limit. The thread ends with the test.
fn read_in_thread(mut master: File, paused: Arc<AtomicBool>) -> Receiver<Vec<u8>> {
    let (sender, output) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = vec![0u8; 65536];
        loop {
            if paused.load(Ordering::Relaxed) {
                thread::sleep(READ_INTERVAL);
                continue;
            }
            // The read ends with an error when the test closes the terminal.
            match master.read(&mut buffer) {
                Ok(count @ 1..) if sender.send(buffer[..count].to_vec()).is_ok() => {}
                _ => return,
            }
        }
    });
    output
}

fn size(rows: u16, columns: u16) -> Size {
    Size::new(rows, columns).expect("the size of a window is not zero")
}

fn window(rows: u16, columns: u16) -> Winsize {
    Winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}
