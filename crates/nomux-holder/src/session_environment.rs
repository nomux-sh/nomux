//! The environment of the command of a session: the environment of the
//! holder, with the changes of this module. The rule is in
//! `docs/daemon.md`, section "Environment of a session".
//!
//! The variables of the session let a program in the session tell the
//! daemon which session it is in, for example for `nomux status set`.

use std::env;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::DAEMON_SOCKET_FILE;
use nomux_holder_protocol::wire::SessionToken;

/// The variable with the session identifier.
const SESSION_ID: &str = "NOMUX_SESSION_ID";
/// The variable with the path of the socket of the daemon.
const DAEMON_SOCKET: &str = "NOMUX_SOCK";
/// The variable with the token of the session.
const TOKEN: &str = "NOMUX_TOKEN";

/// The terminal of a session is the terminal of nomux, which follows
/// xterm, and not the terminal of a client.
const TERMINAL_TYPE: (&str, &str) = ("TERM", "xterm-256color");
const COLOR_SUPPORT: (&str, &str) = ("COLORTERM", "truecolor");

/// The variables that select the character set, in the sequence in which
/// the C library reads them.
const LOCALE_VARIABLES: [&str; 3] = ["LC_ALL", "LC_CTYPE", "LANG"];
#[cfg(target_os = "macos")]
const UTF8_CHARACTER_TYPE: &str = "UTF-8";
#[cfg(not(target_os = "macos"))]
const UTF8_CHARACTER_TYPE: &str = "C.UTF-8";

/// The variables that describe the terminal of the process that started
/// the daemon. They are false in a session.
const STARTER_TERMINAL_VARIABLES: [&str; 4] =
    ["COLUMNS", "LINES", "TERM_PROGRAM", "TERM_PROGRAM_VERSION"];

/// The changes to the environment of the holder for the command.
pub(crate) struct CommandEnvironment {
    set: Vec<(&'static str, OsString)>,
}

impl CommandEnvironment {
    /// The environment of the command of a session. The daemon listens in
    /// the runtime directory of the holder.
    pub(crate) fn of_session(
        session_id: &SessionId,
        runtime_directory: &Path,
        token: &SessionToken,
    ) -> Self {
        let mut set = vec![
            (SESSION_ID, session_id.as_str().into()),
            (
                DAEMON_SOCKET,
                runtime_directory.join(DAEMON_SOCKET_FILE).into(),
            ),
            (TOKEN, token.as_str().into()),
            (TERMINAL_TYPE.0, TERMINAL_TYPE.1.into()),
            (COLOR_SUPPORT.0, COLOR_SUPPORT.1.into()),
        ];
        if !has_locale() {
            set.push(("LC_CTYPE", UTF8_CHARACTER_TYPE.into()));
        }
        Self { set }
    }

    pub(crate) fn apply(self, command: &mut Command) {
        for name in STARTER_TERMINAL_VARIABLES {
            command.env_remove(name);
        }
        command.envs(self.set);
    }
}

/// `true` if a variable of the locale has a value. Without one, the C
/// library uses the locale `C`, which has no characters outside ASCII.
fn has_locale() -> bool {
    LOCALE_VARIABLES
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// Makes the token of this start of the session from random bytes of the
/// operating system.
pub(crate) fn new_token() -> io::Result<SessionToken> {
    let mut random = [0; SessionToken::RANDOM_LENGTH];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    Ok(SessionToken::from_random(random))
}
