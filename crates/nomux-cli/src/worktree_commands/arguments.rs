//! The command line of the `worktree` commands.

use clap::{Args, Subcommand, ValueEnum};

#[derive(Debug, Subcommand)]
pub(crate) enum WorktreeCommand {
    /// List the worktrees of a Project: their state, their size and why each one can or cannot
    /// be removed. The command only reads
    Ls {
        #[command(flatten)]
        projects: ProjectSelection,

        /// Wait until each size is measured. Without it, a size that is not measured yet or is
        /// old is marked, and the daemon measures it in the background
        #[arg(long)]
        wait: bool,

        /// Run `git fetch --prune` in each Project first: the only command of nomux that uses the
        /// network. Only on the local host
        #[arg(long)]
        fetch: bool,
    },
    /// Remove build output and move finished worktrees to the trash place. Without --yes, the
    /// command shows the plan: each path, its size and the reason, and asks on a terminal
    Clean(CleanArguments),
    /// Move a worktree back from the trash place during its grace time
    Restore {
        /// The identifier of the trash entry, or the path or the name of the worktree
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// The trash place of the worktrees
    Trash {
        #[command(subcommand)]
        command: TrashCommand,
    },
    /// Mark a worktree as finished, or to keep: no clean changes a kept worktree
    Mark {
        /// The worktree: its path or its name
        #[arg(value_name = "WORKTREE")]
        worktree: String,

        /// The mark
        #[arg(value_enum)]
        mark: MarkValue,
    },
    /// Remove the mark of a worktree
    Unmark {
        /// The worktree: its path or its name
        #[arg(value_name = "WORKTREE")]
        worktree: String,
    },
}

/// The Projects of a command.
#[derive(Debug, Args)]
pub(crate) struct ProjectSelection {
    /// The Project: its identifier or its path [default: the Project of the current directory]
    #[arg(value_name = "NODE", conflicts_with = "all")]
    pub(crate) project: Option<String>,

    /// Each Project
    #[arg(long)]
    pub(crate) all: bool,
}

#[derive(Debug, Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag of the command line is one option"
)]
pub(crate) struct CleanArguments {
    #[command(flatten)]
    pub(crate) projects: ProjectSelection,

    /// Remove the build output of each linked worktree. The directories and the branches stay
    #[arg(long, group = "action")]
    pub(crate) build_output: bool,

    /// Remove the build output of each finished worktree, then move it to the trash place, and
    /// prune the records of missing directories [default]
    #[arg(long, group = "action")]
    pub(crate) finished: bool,

    /// Remove the build output of each stale worktree
    #[arg(long, group = "action")]
    pub(crate) stale: bool,

    /// Only prune the records of git of worktrees whose directory is missing
    #[arg(long, group = "action")]
    pub(crate) prune: bool,

    /// Only these worktrees: a path or a name. A main worktree gets its build output removed only
    /// when it is named here
    #[arg(long = "worktree", value_name = "WORKTREE")]
    pub(crate) worktrees: Vec<String>,

    /// Also delete the branch of each worktree that the base branch has completely, at the end
    /// of the grace time
    #[arg(long)]
    pub(crate) delete_branch: bool,

    /// Also move a worktree with commits that are not pushed. Its branch keeps the commits
    #[arg(long)]
    pub(crate) allow_unpushed: bool,

    /// Also move the ignored files that are not build output, for example `.env`. They are
    /// deleted at the end of the grace time
    #[arg(long)]
    pub(crate) include_ignored: bool,

    /// Remove now, without the grace time. Also for a worktree that has no trash place
    #[arg(long)]
    pub(crate) now: bool,

    /// Show the plan and change nothing
    #[arg(long, conflicts_with = "yes")]
    pub(crate) dry_run: bool,

    /// Clean without a question
    #[arg(long)]
    pub(crate) yes: bool,

    /// Run `git fetch --prune` in each Project first. Only on the local host
    #[arg(long)]
    pub(crate) fetch: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TrashCommand {
    /// List the worktrees in the trash place
    Ls {
        /// Only the entries of this Project: its identifier or its path [default: each Project]
        #[arg(value_name = "NODE")]
        project: Option<String>,
    },
    /// Remove worktrees from the trash place now, with the same checks as at the end of the
    /// grace time
    Empty {
        /// The identifiers of the entries [default: each entry]
        #[arg(value_name = "ID")]
        ids: Vec<String>,

        /// Remove without a question
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum MarkValue {
    /// The work is done: the worktree is finished
    Done,
    /// Keep the worktree: no clean changes it
    Keep,
}
