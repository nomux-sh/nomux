//! The agent tool that probably made a worktree, from the directory where
//! the tool puts its worktrees. The table is data: a new tool is one row.

use std::env;
use std::path::Path;

/// A part of the path of a worktree and the tool that makes worktrees
/// below a directory with that part.
const TOOL_DIRECTORIES: &[(&str, &str)] = &[
    ("/.claude/worktrees/", "claude-code"),
    ("/.codex/worktrees/", "codex"),
    ("/.cursor/worktrees/", "cursor"),
    ("/orca/workspaces/", "orca"),
];

/// The tool of the worktree at `path`. Empty when no row matches. Codex
/// puts its worktrees in `$CODEX_HOME/worktrees` when the variable is set.
pub(super) fn tool_of(path: &str) -> &'static str {
    let codex_home = env::var_os("CODEX_HOME");
    if codex_home
        .is_some_and(|home| Path::new(path).starts_with(Path::new(&home).join("worktrees")))
    {
        return "codex";
    }
    TOOL_DIRECTORIES
        .iter()
        .find(|(part, _)| path.contains(part))
        .map_or("", |(_, tool)| tool)
}
