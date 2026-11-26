//! Prints the result of a command: a report on the standard output, an
//! error on the standard error.

mod extension_human;
mod extension_json;
mod hooks_human;
mod hooks_json;
mod human;
mod json;
mod memory_human;
mod memory_json;
mod memory_size;
mod status_human;
mod status_json;

use std::io::{self, Write};

use nomux_client::{ClientError, HostEvent};

use crate::arguments::Arguments;
use crate::hook_commands::HooksReport;
use crate::report::Report;

/// The form of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// Short text for a person.
    Human,
    /// Only the part of the result that a script needs.
    Quiet,
    /// One JSON document with a schema version.
    Json,
}

impl Format {
    pub(crate) fn of(arguments: &Arguments) -> Self {
        match (arguments.json, arguments.quiet) {
            (true, _) => Self::Json,
            (false, true) => Self::Quiet,
            (false, false) => Self::Human,
        }
    }
}

pub(crate) fn print_report(report: &Report, format: Format) -> io::Result<()> {
    // The answer of a hook is for its tool, in the form that the tool reads.
    if let Report::Hooks(HooksReport::Answered { output }) = report {
        return io::stdout().lock().write_all(output);
    }
    if let (Some(payload), Format::Human | Format::Quiet) = (report.call_payload(), format) {
        return io::stdout().lock().write_all(payload);
    }
    let text = match (format, report) {
        // The output of a session is bytes, and not each byte sequence is
        // text.
        (Format::Human | Format::Quiet, Report::OutputCaptured { output, .. }) => {
            return io::stdout().lock().write_all(output);
        }
        (Format::Human, _) => human::report(report, human::Style::of_standard_output()),
        (Format::Quiet, _) => human::quiet_report(report),
        (Format::Json, _) => json::report(report),
    };
    io::stdout().lock().write_all(text.as_bytes())
}

/// The line of one event of `events --follow`.
pub(crate) fn event_line(event: &HostEvent, format: Format) -> String {
    match format {
        Format::Human | Format::Quiet => status_human::event_line(event),
        Format::Json => json::event(event),
    }
}

pub(crate) fn print_error(error: &ClientError, code: u8, format: Format) -> io::Result<()> {
    let text = match format {
        Format::Human | Format::Quiet => format!("nomux: {error}\n"),
        Format::Json => json::error(error, code),
    };
    io::stderr().lock().write_all(text.as_bytes())
}
