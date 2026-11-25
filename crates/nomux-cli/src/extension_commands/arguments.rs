//! The command line of the `ext` commands.

use std::path::PathBuf;

use clap::Subcommand;

use super::grant::Grant;

#[derive(Debug, Subcommand)]
pub(crate) enum ExtCommand {
    /// List the installed extensions
    Ls,
    /// Install an extension from a directory on the host. Without --grant or
    /// --grant-all, print the permissions that it asks for and install nothing
    Install {
        /// The directory of the package, which has the file extension.toml
        directory: PathBuf,

        /// Approve one permission that the extension asks for: fs=<directory>,
        /// process=<program>, network or scope=<scope>
        #[arg(long, value_name = "PERMISSION")]
        grant: Vec<Grant>,

        /// Approve each permission that the extension asks for, but not a
        /// directory that starts with ${worktree}
        #[arg(long)]
        grant_all: bool,
    },
    /// Stop an extension and remove its package, its state and its data
    Rm {
        /// The identifier of the extension
        id: String,
    },
    /// Enable an extension, also after the daemon disabled it because of failures
    Enable {
        /// The identifier of the extension
        id: String,
    },
    /// Disable an extension and stop its process
    Disable {
        /// The identifier of the extension
        id: String,
    },
    /// Call a method of an extension and write the payload of its result to
    /// the standard output
    Call {
        /// The identifier of the extension
        id: String,

        /// The name of the method
        method: String,

        /// The payload of the call, in the encoding of the method: a text, or
        /// @<file> for the content of a file [default: an empty payload]
        #[arg(long, value_name = "TEXT|@FILE", allow_hyphen_values = true)]
        payload: Option<String>,
    },
    /// Check the extension.toml of a directory on this computer. The command
    /// needs no daemon
    Validate {
        /// The directory of the package, which has the file extension.toml
        directory: PathBuf,
    },
}
