//! The command line of the `worktree` commands.

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum WorktreeCommand {
    /// List the worktrees of a Project: their state, their size and why each one can or cannot
    /// be removed. The command only reads
    Ls {
        /// The Project: its identifier or its path [default: the Project of the current
        /// directory]
        #[arg(value_name = "NODE", conflicts_with = "all")]
        project: Option<String>,

        /// List the worktrees of each Project
        #[arg(long)]
        all: bool,

        /// Wait until each size is measured. Without it, a size that is not measured yet or is
        /// old is marked, and the daemon measures it in the background
        #[arg(long)]
        wait: bool,
    },
}
