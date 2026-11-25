//! The command line of the daemon.

use std::ffi::OsString;

pub(crate) const USAGE: &str = "\
Usage: nomuxd [--detach] [--replace]
       nomuxd --version

Without an option, nomuxd runs in the foreground and writes its log to the
standard error.

Options:
  --detach   Start the daemon as a separate process and end. The log of that
             daemon goes to nomuxd.log in the runtime directory (~/.nomux/run).
  --replace  If a daemon already runs, stop it and take its place. No session
             ends.

Environment:
  NOMUX_LOG  The log level: error, warn (default), info, debug or trace
  NOMUX_EXTENSION_IDLE_SECONDS
             The time without a call after which the daemon stops the process
             of an extension (default: 60)
";

/// The name of the option that [`Invocation::Detach`] gives to the daemon
/// that it starts.
pub(crate) const REPLACE_OPTION: &str = "--replace";

/// What a new daemon does when a daemon already runs for the runtime
/// directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunningDaemon {
    /// The new daemon ends with an error.
    Keep,
    /// The new daemon stops the daemon that runs and takes its place.
    Replace,
}

/// What the command line asks the daemon to do.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Invocation {
    Version,
    Detach(RunningDaemon),
    Serve(RunningDaemon),
}

pub(crate) fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Invocation, String> {
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    if let [option] = arguments.as_slice()
        && (option == "--version" || option == "-V")
    {
        return Ok(Invocation::Version);
    }
    let mut detach = false;
    let mut running_daemon = RunningDaemon::Keep;
    for option in &arguments {
        if option == "--detach" {
            detach = true;
        } else if option == REPLACE_OPTION {
            running_daemon = RunningDaemon::Replace;
        } else {
            return Err(format!("unknown option {}", option.display()));
        }
    }
    Ok(if detach {
        Invocation::Detach(running_daemon)
    } else {
        Invocation::Serve(running_daemon)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> Result<Invocation, String> {
        parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn replace_combines_with_detach_and_version_stands_alone() {
        assert_eq!(parsed(&[]), Ok(Invocation::Serve(RunningDaemon::Keep)));
        assert_eq!(
            parsed(&["--replace", "--detach"]),
            Ok(Invocation::Detach(RunningDaemon::Replace))
        );
        assert_eq!(
            parsed(&["--replace"]),
            Ok(Invocation::Serve(RunningDaemon::Replace))
        );
        assert_eq!(parsed(&["--version"]), Ok(Invocation::Version));
        assert!(parsed(&["--detach", "--version"]).is_err());
    }
}
