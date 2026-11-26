//! The command line of the `project` and `group` commands.

use std::path::PathBuf;

use clap::{ArgGroup, Subcommand};

/// A node on the command line: its identifier, or the names from the top
/// level down to it with `/` between them.
const NODE: &str = "NODE";

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommand {
    /// Add a directory of the host as a Project. In a git repository on the local host, the
    /// Project is the root of the repository
    Add {
        /// The directory [default: the current directory; with --host, it is necessary and
        /// absolute]
        directory: Option<PathBuf>,

        /// The name of the Project [default: the last part of the directory]
        #[arg(long)]
        name: Option<String>,

        /// The Group of the Project [default: the top level]
        #[arg(long, value_name = NODE)]
        group: Option<String>,
    },
    /// Remove a Project from the workspace tree. No file and no session changes
    Rm {
        /// The Project: its identifier or its path, for example `Client work/Acme/acme-web`
        #[arg(value_name = NODE)]
        project: String,
    },
    /// List the Projects
    Ls,
}

#[derive(Debug, Subcommand)]
pub(crate) enum GroupCommand {
    /// Make an empty Group
    New {
        /// The name of the Group
        name: String,

        /// The Group of the new Group [default: the top level]
        #[arg(long = "in", value_name = NODE)]
        parent: Option<String>,
    },
    /// Give a Group or a Project a new name
    Rename {
        /// The Group or the Project: its identifier or its path
        #[arg(value_name = NODE)]
        node: String,

        /// The new name
        name: String,
    },
    /// Put a Group or a Project in a different Group, at the top level, or at a different
    /// position
    #[command(group(
        ArgGroup::new("place").required(true).multiple(true).args(["to", "top", "before", "after"])
    ))]
    Move {
        /// The Group or the Project: its identifier or its path
        #[arg(value_name = NODE)]
        node: String,

        /// The new Group
        #[arg(long, value_name = NODE, conflicts_with = "top")]
        to: Option<String>,

        /// Put the node at the top level
        #[arg(long)]
        top: bool,

        /// Put the node before this sibling
        #[arg(long, value_name = NODE, conflicts_with = "after")]
        before: Option<String>,

        /// Put the node after this sibling
        #[arg(long, value_name = NODE)]
        after: Option<String>,
    },
    /// Remove a Group. Its content takes its place: no Project and no Group in it goes away
    Rm {
        /// The Group: its identifier or its path
        #[arg(value_name = NODE)]
        group: String,
    },
    /// List the Groups
    Ls {
        /// Show the full tree: each Group with its Groups and Projects
        #[arg(long)]
        tree: bool,
    },
}
