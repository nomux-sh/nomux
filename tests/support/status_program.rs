//! A program that reports its status with the Program Status Protocol
//! (OSC 7501), as an agent tool does. A test starts it as the command of a
//! session.
//!
//! The program first asks the terminal if it reads the reports, with the
//! query `OSC 7501 ; ? ST`, and waits for the answer. It then prints
//! `ready`. Each line that it reads after that is the payload of one
//! report: the program writes `OSC 7501 ; <line> ST`. It ends at the end of
//! its input.

use std::io::{self, BufRead, Read, Write};

use nomux_term::osc::program_status::QUERY_REPLY;
use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};

fn main() {
    let stdin = io::stdin();
    let normal = tcgetattr(&stdin).expect("the input is a terminal");
    // The answer of the terminal has no line end: the program reads it byte
    // by byte, and the terminal does not echo it.
    let mut raw = normal.clone();
    raw.local_modes &= !(LocalModes::ICANON | LocalModes::ECHO);
    tcsetattr(&stdin, OptionalActions::Now, &raw).expect("the terminal takes the mode");

    let mut stdout = io::stdout();
    stdout
        .write_all(b"\x1b]7501;?\x1b\\")
        .and_then(|()| stdout.flush())
        .expect("the query can be written");
    let mut answer = Vec::new();
    let mut byte = [0];
    while !answer.ends_with(QUERY_REPLY) {
        stdin
            .lock()
            .read_exact(&mut byte)
            .expect("the terminal answers");
        answer.push(byte[0]);
    }

    let mut lines_without_echo = normal;
    lines_without_echo.local_modes &= !LocalModes::ECHO;
    tcsetattr(&stdin, OptionalActions::Now, &lines_without_echo)
        .expect("the terminal takes the mode");
    println!("ready");
    for line in stdin.lock().lines() {
        let line = line.expect("the input is text");
        print!("\x1b]7501;{}\x1b\\", line.trim_end());
        println!("reported");
    }
}
