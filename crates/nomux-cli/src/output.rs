//! Prints the result of a command: a report on the standard output, an
//! error on the standard error.

mod human;
mod json;

use std::io::{self, Write};

use nomux_client::ClientError;

use crate::arguments::Arguments;
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
    let text = match format {
        Format::Human => human::report(report, human::Style::of_standard_output()),
        Format::Quiet => human::quiet_report(report),
        Format::Json => json::report(report),
    };
    io::stdout().lock().write_all(text.as_bytes())
}

pub(crate) fn print_error(error: &ClientError, code: u8, format: Format) -> io::Result<()> {
    let text = match format {
        Format::Human | Format::Quiet => format!("nomux: {error}\n"),
        Format::Json => json::error(error, code),
    };
    io::stderr().lock().write_all(text.as_bytes())
}
