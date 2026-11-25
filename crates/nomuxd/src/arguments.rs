//! The command line of the daemon.

use std::ffi::OsString;

pub(crate) const USAGE: &str = "\
Usage: nomuxd [--detach]
       nomuxd --version

Without an option, nomuxd runs in the foreground and writes its log to the
standard error.

Options:
  --detach  Start the daemon as a separate process and end. The log of that
            daemon goes to nomuxd.log in the runtime directory (~/.nomux/run).

Environment:
  NOMUX_LOG  The log level: error, warn (default), info, debug or trace
";

/// What the command line asks the daemon to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation {
    Version,
    Detach,
    Serve,
}

pub(crate) fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Invocation, String> {
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    match arguments.as_slice() {
        [] => Ok(Invocation::Serve),
        [option] if option == "--detach" => Ok(Invocation::Detach),
        [option] if option == "--version" || option == "-V" => Ok(Invocation::Version),
        [option, ..] => Err(format!("unknown option {}", option.display())),
    }
}
