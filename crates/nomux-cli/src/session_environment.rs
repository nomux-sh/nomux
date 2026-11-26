//! The variables that the holder gives each process of a session. With
//! them, a program in a session proves to the daemon which session it is in.

/// The identifier of the session.
pub(crate) const SESSION_ID: &str = "NOMUX_SESSION_ID";
/// The socket of the daemon of the session.
pub(crate) const DAEMON_SOCKET: &str = "NOMUX_SOCK";
/// The token of the session.
pub(crate) const TOKEN: &str = "NOMUX_TOKEN";

/// The value of the variable `name`. `None` if it is not set or empty.
pub(crate) fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}
