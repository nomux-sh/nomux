//! A process that uses a known amount of memory, as the command of a
//! session in a test.
//!
//! ```text
//! nomux-test-memory-user <ready file> <mebibytes> [<mebibytes>...]
//! ```
//!
//! The process allocates the first number of mebibytes and writes to each
//! page. With more numbers, it starts a child with the other numbers.
//! The last process of the chain makes `<ready file>`: each process has
//! its memory then. No process ends by itself.

use std::env;
use std::fs;
use std::hint::black_box;
use std::process::Command;
use std::thread;

const MEBIBYTE: usize = 1024 * 1024;

fn main() {
    let mut arguments = env::args().skip(1);
    let ready_file = arguments.next().expect("the path of the ready file");
    let mebibytes: usize = arguments
        .next()
        .and_then(|number| number.parse().ok())
        .expect("a number of mebibytes");
    // A value that is not zero: the operating system gives a page of zeros
    // no memory.
    let memory = black_box(vec![1u8; mebibytes * MEBIBYTE]);

    let for_children: Vec<String> = arguments.collect();
    if for_children.is_empty() {
        fs::write(&ready_file, "").expect("the ready file can be made");
    } else {
        let program = env::current_exe().expect("this program has a path");
        let mut child = Command::new(program)
            .arg(&ready_file)
            .args(for_children)
            .spawn()
            .expect("the child starts");
        child.wait().expect("the child can be waited for");
    }
    loop {
        thread::park();
        black_box(&memory);
    }
}
