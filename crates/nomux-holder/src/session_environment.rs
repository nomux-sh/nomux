//! The variables that the holder adds to the environment of the command of
//! its session. A program in the session uses them to tell the daemon which
//! session it is in, for example for `nomux status set`.

use std::ffi::OsString;
use std::io;
use std::path::Path;

use nomux_holder_protocol::SessionId;
use nomux_holder_protocol::discovery::DAEMON_SOCKET_FILE;
use nomux_holder_protocol::wire::SessionToken;

/// The variable with the session identifier.
const SESSION_ID: &str = "NOMUX_SESSION_ID";
/// The variable with the path of the socket of the daemon.
const DAEMON_SOCKET: &str = "NOMUX_SOCK";
/// The variable with the token of the session.
const TOKEN: &str = "NOMUX_TOKEN";

/// Makes the token of this start of the session from random bytes of the
/// operating system.
pub(crate) fn new_token() -> io::Result<SessionToken> {
    let mut random = [0; SessionToken::RANDOM_LENGTH];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    Ok(SessionToken::from_random(random))
}

/// The variables of a session, for the environment of its command. The
/// daemon listens in the runtime directory of the holder.
pub(crate) fn variables(
    session_id: &SessionId,
    runtime_directory: &Path,
    token: &SessionToken,
) -> [(&'static str, OsString); 3] {
    [
        (SESSION_ID, session_id.as_str().into()),
        (
            DAEMON_SOCKET,
            runtime_directory.join(DAEMON_SOCKET_FILE).into(),
        ),
        (TOKEN, token.as_str().into()),
    ]
}
