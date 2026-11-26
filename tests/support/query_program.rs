//! A program that sends queries to its terminal, as a full-screen program
//! does at its start. A test starts it as the command of a session.
//!
//! With the argument `--after-key`, the program first waits for one key.
//! It then sends the primary device attributes query (`CSI c`) and the
//! cursor position query (`CSI 6 n`), and reads its input for one second.
//! It writes to the file of its first argument the milliseconds until the
//! two answers were there, or `none`, a line break, and all the bytes that
//! it read. Then it ends.

use std::fs;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use rustix::termios::{OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr};

const READ_TIME: Duration = Duration::from_secs(1);

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result_file = arguments.first().expect("the first argument is a file");
    let after_key = arguments.iter().any(|argument| argument == "--after-key");

    let stdin = io::stdin();
    let mut raw = tcgetattr(&stdin).expect("the input is a terminal");
    raw.make_raw();
    // A read returns after a tenth of a second without input.
    raw.special_codes[SpecialCodeIndex::VMIN] = 0;
    raw.special_codes[SpecialCodeIndex::VTIME] = 1;
    tcsetattr(&stdin, OptionalActions::Now, &raw).expect("the terminal takes the mode");
    let mut input = stdin.lock();
    let mut buffer = [0; 256];
    while after_key && input.read(&mut buffer).expect("the input can be read") == 0 {}

    let mut stdout = io::stdout();
    stdout
        .write_all(b"\x1b[c\x1b[6n")
        .and_then(|()| stdout.flush())
        .expect("the queries can be written");
    let start = Instant::now();
    let mut answers = Vec::new();
    let mut answered = None;
    while start.elapsed() < READ_TIME {
        let count = input.read(&mut buffer).expect("the input can be read");
        answers.extend_from_slice(&buffer[..count]);
        if answered.is_none() && answers.contains(&b'c') && answers.contains(&b'R') {
            answered = Some(start.elapsed());
        }
    }
    let time = answered.map_or_else(|| "none".to_owned(), |time| time.as_millis().to_string());
    let mut result = format!("{time}\n").into_bytes();
    result.extend_from_slice(&answers);
    fs::write(result_file, result).expect("the result can be written");
}
