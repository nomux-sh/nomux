//! The guard of the main socket of the daemon: a process of a sandboxed
//! session must not control the daemon, and thus the other sessions, through
//! it. `docs/sandbox.md`, "Socket guard", has the rule.
//!
//! - On a kernel with the Unix socket rule of Landlock (interface version
//!   9), the sandbox cannot connect to the main socket at all: the plan
//!   permits the agent socket only.
//! - On an older kernel, the connect works. The daemon then reads the
//!   process ID of the peer and refuses the connection if the process
//!   descends from the command of a sandboxed session. A process that left
//!   the tree, for example a daemon that the command started, escapes this
//!   check: `unix_sockets` is then a missing feature of the session.

use std::collections::HashSet;

use nomux_holder_protocol::discovery::ProcessIdentity;

/// `true` if the peer `pid` is a process of a sandboxed session: one of the
/// commands `roots` or a process that descends from one. A peer without a
/// process ID is refused if a sandboxed session runs.
pub(crate) fn refuses(roots: &HashSet<u32>, pid: Option<u32>) -> bool {
    if roots.is_empty() {
        return false;
    }
    let Some(mut pid) = pid else {
        return true;
    };
    // The chain of parents ends at the first process. The count limits a
    // walk in a table that changes during it.
    for _ in 0..4096 {
        if roots.contains(&pid) {
            return true;
        }
        match ProcessIdentity::with_parent_of(pid) {
            Ok(Some((_, parent))) if parent != 0 && parent != pid => pid = parent,
            _ => return false,
        }
    }
    false
}
