//! The checks of the worktrees of a host: the free space of the file systems
//! of the Projects, and the advice for build output that the worktrees can
//! share. The checks only read the answer of `Worktree.List`.

use crate::doctor::HostCheck;
use crate::{ProjectWorktrees, Worktree};

const GIBIBYTE: u64 = 1 << 30;
/// Below this free space, the `free-space` check warns.
const WARNING_FREE_BYTES: u64 = 10 * GIBIBYTE;
/// Below this free space, the `free-space` check fails.
const CRITICAL_FREE_BYTES: u64 = 3 * GIBIBYTE;

/// The rule identifier of the build output of cargo.
const CARGO_TARGET: &str = "cargo-target";
/// The rule identifier of the packages of npm, pnpm and bun.
const NODE_MODULES: &str = "node-modules";

/// The `free-space` and the `shared-build-output` checks. No check without a
/// Project.
pub(crate) fn checks(projects: &[ProjectWorktrees]) -> Vec<HostCheck> {
    if projects.is_empty() {
        return Vec::new();
    }
    vec![free_space(projects), shared_build_output(projects)]
}

fn free_space(projects: &[ProjectWorktrees]) -> HostCheck {
    let Some(free) = projects
        .iter()
        .filter_map(|project| project.free_bytes)
        .min()
    else {
        return HostCheck::of(
            "free-space",
            Ok("the daemon does not know the free space".to_owned()),
        );
    };
    let text = format!("{} free on the file system of a Project", size_text(free));
    let result = if free < CRITICAL_FREE_BYTES {
        Err(format!(
            "{text}: below the critical level of {}. `nomux worktree ls --all` shows what the \
             worktrees hold",
            size_text(CRITICAL_FREE_BYTES)
        ))
    } else if free < WARNING_FREE_BYTES {
        Ok(format!(
            "warning: {text}: below the warning level of {}. `nomux worktree ls --all` shows \
             what the worktrees hold",
            size_text(WARNING_FREE_BYTES)
        ))
    } else {
        Ok(text)
    };
    HostCheck::of("free-space", result)
}

fn shared_build_output(projects: &[ProjectWorktrees]) -> HostCheck {
    let mut advice = Vec::new();
    let cargo = BuildOutputUse::of(projects, CARGO_TARGET);
    if cargo.worktrees >= 2 {
        advice.push(format!(
            "{} worktrees build their own cargo target directory ({}). With one shared build \
             directory, a second worktree of a measured project built in 0.55 s instead of 17.4 \
             s and added 10 MiB instead of 354 MiB: set `CARGO_BUILD_BUILD_DIR=<a directory \
             for the project>` for its sessions, or `build.build-dir` in ~/.cargo/config.toml. \
             Builds in two worktrees then wait for one lock",
            cargo.worktrees,
            size_text(cargo.bytes)
        ));
    }
    let node = BuildOutputUse::of(projects, NODE_MODULES);
    if node.worktrees >= 2 {
        advice.push(format!(
            "{} worktrees install their own node_modules ({}). pnpm and bun link the files of \
             a package from one store, npm copies them",
            node.worktrees,
            size_text(node.bytes)
        ));
    }
    let detail = if advice.is_empty() {
        "no advice: no build output that the worktrees can share".to_owned()
    } else {
        advice.join(". ")
    };
    HostCheck::of("shared-build-output", Ok(detail))
}

/// The worktrees that have build output of one rule, and its measured
/// bytes.
struct BuildOutputUse {
    worktrees: usize,
    bytes: u64,
}

impl BuildOutputUse {
    fn of(projects: &[ProjectWorktrees], rule: &str) -> Self {
        let sizes: Vec<u64> = projects
            .iter()
            .flat_map(|project| &project.worktrees)
            .filter_map(|worktree: &Worktree| worktree.size.as_ref())
            .filter_map(|size| {
                let outputs = size
                    .build_outputs
                    .iter()
                    .filter(|output| output.rule == rule);
                let mut outputs = outputs.peekable();
                outputs.peek()?;
                Some(outputs.map(|output| output.bytes).sum())
            })
            .collect();
        Self {
            worktrees: sizes.len(),
            bytes: sizes.iter().sum(),
        }
    }
}

/// `bytes` in GiB, or in MiB below 1 GiB, with one decimal place, cut and
/// not rounded.
fn size_text(bytes: u64) -> String {
    let (unit, name) = if bytes < GIBIBYTE {
        (1 << 20, "MiB")
    } else {
        (GIBIBYTE, "GiB")
    };
    let tenths = u128::from(bytes) * 10 / u128::from(unit);
    format!("{}.{} {name}", tenths / 10, tenths % 10)
}
