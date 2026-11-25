//! The extension of the integration tests of the host.
//!
//! The host gives an extension no arguments and no environment, thus the
//! name of the program file selects the behaviour. The name is the
//! identifier of the extension. Some identifiers fail at the start. Each
//! other identifier is an extension with the methods below, and a test
//! selects a failure with the method that it calls.

mod messages;

use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::{self, ExitCode};
use std::thread;
use std::time::Duration;

use nomux_extension::frame::{self, MAX_ENVELOPE_LENGTH};
use nomux_extension::wire::v1::envelope::Kind;
use nomux_extension::wire::v1::{Envelope, Error, Response, response};
use nomux_extension::{Call, CallError, ErrorCode, Extension};

use messages::{ProcessInfo, Text};

fn main() -> ExitCode {
    let id = program_name();
    match id.as_str() {
        "test.crash-at-start" => {
            eprintln!("the test extension cannot start");
            ExitCode::from(3)
        }
        "test.slow-start" => sleep_forever(),
        "test.refuse-version" => refuse_version(),
        // The program has a different identifier than its manifest.
        "test.wrong-identity" => serve(extension("test.other")),
        _ => serve(extension(&id)),
    }
}

fn program_name() -> String {
    let program = env::args_os().next().unwrap_or_default();
    let name = Path::new(&program).file_name().unwrap_or_default();
    name.to_string_lossy().into_owned()
}

fn serve(extension: Extension) -> ExitCode {
    match extension.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("the test extension stopped: {error}");
            ExitCode::FAILURE
        }
    }
}

fn extension(id: &str) -> Extension {
    Extension::new(id, "1.0.0")
        .method("echo", echo)
        .method("fail", fail)
        .method("slow", slow)
        .method("ignore_cancel", ignore_cancel)
        .method("start_ticking", start_ticking)
        .method("ask_host", ask_host)
        .method("process", process_info)
        .method("crash", crash)
        .method("hang", hang)
        .method("bad_frame", bad_frame)
        .method("oversized_frame", oversized_frame)
        .method("flood_stderr", flood_stderr)
}

/// Answers `Hello` as an extension that cannot use the host, without the
/// SDK: the SDK of this workspace accepts the host.
fn refuse_version() -> ExitCode {
    let hello = frame::read_envelope(&mut io::stdin().lock());
    let Ok(Some(Envelope {
        kind: Some(Kind::Request(request)),
    })) = hello
    else {
        return ExitCode::from(2);
    };
    let error = Error {
        code: ErrorCode::VersionIncompatible.into(),
        message: "the extension needs the extension API version ^9".to_owned(),
    };
    let response = Response {
        id: request.id,
        result: Some(response::Result::Error(error)),
    };
    let envelope = Envelope {
        kind: Some(Kind::Response(response)),
    };
    frame::write_envelope(&mut io::stdout().lock(), &envelope).expect("the host reads the output");
    ExitCode::FAILURE
}

fn echo(call: &Call, input: Text) -> Result<Text, CallError> {
    call.host().emit("echoed", &input)?;
    Ok(input)
}

fn fail(_: &Call, _: Text) -> Result<Text, CallError> {
    Err(CallError::failed("the method fails"))
}

/// Runs until the host cancels the call, then tells the host that it saw
/// the cancellation.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the SDK sets the signature of a method"
)]
fn slow(call: &Call, input: Text) -> Result<Text, CallError> {
    while !call.is_cancelled() {
        thread::sleep(Duration::from_millis(5));
    }
    call.host().emit("cancel-seen", &input)?;
    Err(CallError::cancelled())
}

/// Never returns. The extension continues to answer health requests, but it
/// starts no other call.
fn ignore_cancel(_: &Call, _: Text) -> Result<Text, CallError> {
    sleep_forever()
}

/// Starts a thread that sends an event each 50 milliseconds, as an
/// extension that watches files does.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the SDK sets the signature of a method"
)]
fn start_ticking(call: &Call, input: Text) -> Result<Text, CallError> {
    let host = call.host().clone();
    thread::spawn(move || {
        while host.emit("tick", &Text::default()).is_ok() {
            thread::sleep(Duration::from_millis(50));
        }
    });
    Ok(input)
}

/// Calls the host service that the text names and gives its answer back.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the SDK sets the signature of a method"
)]
fn ask_host(call: &Call, input: Text) -> Result<Text, CallError> {
    call.host().call(&input.text, &Text::default())
}

fn process_info(call: &Call, _: Text) -> Result<ProcessInfo, CallError> {
    let directory = env::current_dir().map_err(|error| CallError::failed(error.to_string()))?;
    let variables = env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned());
    Ok(ProcessInfo {
        pid: process::id(),
        working_directory: directory.display().to_string(),
        variables: variables.collect(),
        scopes: call.host().permissions().scopes.clone(),
    })
}

fn crash(_: &Call, _: Text) -> Result<Text, CallError> {
    process::exit(7)
}

/// Holds the lock of the standard output, thus the thread of the SDK that
/// answers health requests cannot write.
fn hang(_: &Call, _: Text) -> Result<Text, CallError> {
    let _output = io::stdout().lock();
    sleep_forever()
}

/// Writes a frame of two bytes that are not an envelope.
fn bad_frame(_: &Call, _: Text) -> Result<Text, CallError> {
    write_to_host(&[0x02, 0xff, 0xff]);
    sleep_forever()
}

/// Writes the length of a frame that is one byte over the limit.
fn oversized_frame(_: &Call, _: Text) -> Result<Text, CallError> {
    let mut length = Vec::new();
    prost::encoding::encode_varint(MAX_ENVELOPE_LENGTH as u64 + 1, &mut length);
    write_to_host(&length);
    sleep_forever()
}

fn flood_stderr(_: &Call, _: Text) -> Result<Text, CallError> {
    let line = vec![b'x'; 64 * 1024];
    while io::stderr().write_all(&line).is_ok() {}
    Err(CallError::failed("the standard error is closed"))
}

fn write_to_host(bytes: &[u8]) {
    let mut output = io::stdout().lock();
    output.write_all(bytes).expect("the host reads the output");
    output.flush().expect("the host reads the output");
}

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}
