//! The command line of a probe.

use std::process::ExitCode;

use crate::candidate::{Parser, Screen};
use crate::corpus::Corpus;
use crate::{conformance, memory, throughput};

const USAGE: &str = "\
Usage: <probe> <command>

Commands:
  name                                   Print the candidate name
  throughput <corpus> <bytes> <runs>     Print the corpus size and the median parse time
  memory <scrollback-lines>              Fill a terminal, print `ready`, wait for end of input
  conformance                            Print `pass` or `fail` for each conformance case
";

enum Command {
    Name,
    Throughput {
        corpus: Corpus,
        bytes: usize,
        runs: usize,
    },
    Memory {
        scrollback_lines: usize,
    },
    Conformance,
}

impl Command {
    fn parse(arguments: &[String]) -> Option<Self> {
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        match arguments[..] {
            ["name"] => Some(Self::Name),
            ["throughput", corpus, bytes, runs] => Some(Self::Throughput {
                corpus: Corpus::from_name(corpus)?,
                bytes: bytes.parse().ok()?,
                runs: runs.parse().ok().filter(|runs| *runs > 0)?,
            }),
            ["memory", scrollback_lines] => Some(Self::Memory {
                scrollback_lines: scrollback_lines.parse().ok()?,
            }),
            ["conformance"] => Some(Self::Conformance),
            _ => None,
        }
    }
}

/// The entry point of a probe for a candidate that has no screen state.
pub fn run_parser<P: Parser>() -> ExitCode {
    run::<P>(|| {
        eprintln!("{} has no screen state", P::NAME);
        ExitCode::FAILURE
    })
}

/// The entry point of a probe for a candidate that keeps the screen state.
pub fn run_screen<S: Screen>() -> ExitCode {
    run::<S>(|| {
        conformance::report::<S>();
        ExitCode::SUCCESS
    })
}

fn run<P: Parser>(conformance: impl FnOnce() -> ExitCode) -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = Command::parse(&arguments) else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    match command {
        Command::Name => println!("{}", P::NAME),
        Command::Throughput {
            corpus,
            bytes,
            runs,
        } => throughput::report::<P>(corpus, bytes, runs),
        Command::Memory { scrollback_lines } => {
            if let Err(error) = memory::hold::<P>(scrollback_lines) {
                eprintln!("{}: {error}", P::NAME);
                return ExitCode::FAILURE;
            }
        }
        Command::Conformance => return conformance(),
    }
    ExitCode::SUCCESS
}
