//! The answers to the queries of a program with the real binaries: the
//! holder answers each query from the terminal state of the session, with
//! no client and with a client whose terminal answers queries too.

use std::fs;
use std::path::Path;

use nomux_end_to_end::{Sandbox, binary, wait_for};

/// The answer of nomux to the primary device attributes query.
const PRIMARY_DEVICE_ATTRIBUTES: &[u8] = b"\x1b[?62;22c";

/// The longest time from the queries to the two answers.
const ANSWER_TIME_MILLISECONDS: u64 = 50;

/// Starts the query program in a new session. It writes its result to
/// `result`.
fn new_query_program(sandbox: &Sandbox, result: &Path, arguments: &[&str]) -> String {
    let program = binary("nomux-test-query-program");
    let program = program.to_str().expect("the path is text");
    let result = result.to_str().expect("the path is text");
    let command = [&["new", "--quiet", "--", program, result], arguments].concat();
    sandbox.nomux_ok(&command).stdout.trim().to_owned()
}

/// The milliseconds until the two answers, and the bytes that the program
/// read.
fn query_result(result: &Path) -> (Option<u64>, Vec<u8>) {
    let contents = wait_for("the result of the query program", || fs::read(result).ok());
    let line_end = contents
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("the result has a line");
    let time = std::str::from_utf8(&contents[..line_end])
        .expect("the time is text")
        .parse()
        .ok();
    (time, contents[line_end + 1..].to_vec())
}

/// Checks that `answers` is one primary device attributes answer and one
/// cursor position report: `CSI row ; column R`.
fn assert_one_answer_to_each_query(answers: &[u8]) {
    let shown = String::from_utf8_lossy(answers);
    let report = answers
        .strip_prefix(PRIMARY_DEVICE_ATTRIBUTES)
        .and_then(|rest| rest.strip_prefix(b"\x1b["))
        .and_then(|rest| rest.strip_suffix(b"R"))
        .unwrap_or_else(|| panic!("not one answer to each query: {shown:?}"));
    let mut numbers = report.split(|byte| *byte == b';');
    let is_number = |part: Option<&[u8]>| {
        part.is_some_and(|digits| !digits.is_empty() && digits.iter().all(u8::is_ascii_digit))
    };
    assert!(
        is_number(numbers.next()) && is_number(numbers.next()) && numbers.next().is_none(),
        "not one cursor position report: {shown:?}"
    );
}

#[test]
fn a_program_without_a_client_gets_the_answers_of_the_session() {
    let sandbox = Sandbox::new();
    let result = sandbox.home().join("answers");
    new_query_program(&sandbox, &result, &[]);

    let (time, answers) = query_result(&result);
    assert_one_answer_to_each_query(&answers);
    let time = time.expect("the two answers came");
    assert!(
        time <= ANSWER_TIME_MILLISECONDS,
        "the answers came after {time} ms"
    );
}

#[test]
fn a_program_with_a_client_gets_each_answer_one_time() {
    let sandbox = Sandbox::new();
    let result = sandbox.home().join("answers");
    let id = new_query_program(&sandbox, &result, &["--after-key"]);

    // The terminal of the client answers each query that it sees.
    let mut client = sandbox.nomux_in_terminal(&["attach", &id]);
    client.type_keys(b"x");

    let (time, answers) = query_result(&result);
    assert_one_answer_to_each_query(&answers);
    assert!(time.is_some(), "the two answers came");
    client.wait_for_exit();
    let output = client.text_output();
    assert!(
        !output.contains("\x1b[c") && !output.contains("\x1b[6n"),
        "the terminal of the client got a query: {output:?}"
    );
}
