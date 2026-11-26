//! The command line of the `hooks` commands.

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum HooksCommand {
    /// Show the change that the hooks of a tool need in its settings file. Write it only with
    /// --apply
    Install {
        /// The name of the tool, as its adapter extension names it
        tool: String,

        /// Print the change and write nothing. This is the default
        #[arg(long, conflicts_with = "apply")]
        dry_run: bool,

        /// Write the change to the settings file of the tool
        #[arg(long)]
        apply: bool,
    },
    /// Show how the removal of the hooks of a tool changes its settings file. Write it only with
    /// --apply
    Uninstall {
        /// The name of the tool
        tool: String,

        /// Print the change and write nothing. This is the default
        #[arg(long, conflicts_with = "apply")]
        dry_run: bool,

        /// Write the change to the settings file of the tool
        #[arg(long)]
        apply: bool,
    },
    /// Show each tool: its adapter, and if its hooks are installed, missing or different
    Status,
}
