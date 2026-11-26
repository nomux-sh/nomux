//! The walk that measures the size of the worktrees of one Project. It runs
//! on a thread that can wait, never on the path of a request.
//!
//! - The size is the allocated size: `st_blocks` × 512.
//! - The walk follows no symbolic link and does not go into a different file
//!   system.
//! - A file with more than one hard link counts one time in the walk of a
//!   Project: the first worktree that the walk measures gets its bytes.
//! - A nested worktree is not in the size of the worktree around it.
//! - After each 2000 entries, the walk pauses for three times the time that
//!   it worked: it uses a quarter of one processor at most.

use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Instant;

/// The walk pauses after this number of entries.
const ENTRIES_BETWEEN_PAUSES: u32 = 2000;
/// The pause is this number of times the time of the work before it.
const PAUSE_FACTOR: u32 = 3;
/// The unit of `st_blocks`.
const BLOCK_BYTES: u64 = 512;

/// One worktree to measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WalkTarget {
    pub(super) root: PathBuf,
    /// The build output directories, relative to `root`, with the identifier
    /// of their rule.
    pub(super) build_outputs: Vec<(String, &'static str)>,
}

/// The size of one worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Measured {
    pub(super) total_bytes: u64,
    /// Each build output directory: its relative path, its rule and its
    /// bytes, in the sequence of `WalkTarget::build_outputs`.
    pub(super) build_outputs: Vec<(String, &'static str, u64)>,
}

/// Measures each target. The result has one entry for each target, in the
/// same sequence.
pub(super) fn walk_project(targets: &[WalkTarget]) -> Vec<Measured> {
    let roots: HashSet<&Path> = targets.iter().map(|target| target.root.as_path()).collect();
    let mut walk = Walk {
        roots,
        hard_links: HashSet::new(),
        pacing: Pacing::new(),
    };
    targets.iter().map(|target| walk.measure(target)).collect()
}

struct Walk<'a> {
    /// The root of each worktree of the Project.
    roots: HashSet<&'a Path>,
    /// The device and the inode of each file with more than one link that
    /// the walk counted.
    hard_links: HashSet<(u64, u64)>,
    pacing: Pacing,
}

impl Walk<'_> {
    fn measure(&mut self, target: &WalkTarget) -> Measured {
        let mut measured = Measured {
            total_bytes: 0,
            build_outputs: target
                .build_outputs
                .iter()
                .map(|(path, rule)| (path.clone(), *rule, 0))
                .collect(),
        };
        let Ok(root) = fs::symlink_metadata(&target.root) else {
            return measured;
        };
        if !root.is_dir() {
            return measured;
        }
        let device = root.dev();
        measured.total_bytes += allocated(&root);
        // Each directory to read, with the index of the build output
        // directory that contains it.
        let mut stack: Vec<(PathBuf, Option<usize>)> = vec![(target.root.clone(), None)];
        while let Some((directory, output)) = stack.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                self.pacing.count_entry();
                // On Unix, the metadata of an entry does not follow a link.
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if metadata.dev() != device {
                    continue;
                }
                let path = entry.path();
                let mut output = output;
                if metadata.is_dir() {
                    if self.roots.contains(path.as_path()) {
                        continue;
                    }
                    output = output.or_else(|| build_output_index(target, &path));
                    stack.push((path, output));
                } else if metadata.nlink() > 1
                    && !self.hard_links.insert((metadata.dev(), metadata.ino()))
                {
                    continue;
                }
                let bytes = allocated(&metadata);
                measured.total_bytes += bytes;
                if let Some(index) = output {
                    measured.build_outputs[index].2 += bytes;
                }
            }
        }
        measured
    }
}

fn build_output_index(target: &WalkTarget, path: &Path) -> Option<usize> {
    let relative = path.strip_prefix(&target.root).ok()?;
    target
        .build_outputs
        .iter()
        .position(|(output, _)| Path::new(output) == relative)
}

fn allocated(metadata: &Metadata) -> u64 {
    metadata.blocks() * BLOCK_BYTES
}

/// The pauses of the walk.
struct Pacing {
    entries: u32,
    since: Instant,
}

impl Pacing {
    fn new() -> Self {
        Self {
            entries: 0,
            since: Instant::now(),
        }
    }

    fn count_entry(&mut self) {
        self.entries += 1;
        if self.entries < ENTRIES_BETWEEN_PAUSES {
            return;
        }
        thread::sleep(self.since.elapsed() * PAUSE_FACTOR);
        self.entries = 0;
        self.since = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    const MEBIBYTE: u64 = 1 << 20;
    /// Directories and small files of a test tree take less than this.
    const SMALL: u64 = 256 * 1024;

    fn write_mebibyte(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // No run of equal bytes: the file has no hole.
        let bytes: Vec<u8> = (0..MEBIBYTE)
            .map(|index| (index * 7919 % 251) as u8)
            .collect();
        fs::write(path, bytes).unwrap();
    }

    fn target(root: &Path, build_outputs: &[(&str, &'static str)]) -> WalkTarget {
        WalkTarget {
            root: root.to_owned(),
            build_outputs: build_outputs
                .iter()
                .map(|(path, rule)| ((*path).to_owned(), *rule))
                .collect(),
        }
    }

    #[test]
    fn a_hard_link_counts_one_time_in_a_project_and_a_link_out_and_a_nested_worktree_do_not_count()
    {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        let nested = first.join(".claude/worktrees/nested");
        let outside = directory.path().join("outside.bin");
        write_mebibyte(&first.join("target/debug/program"));
        fs::hard_link(
            first.join("target/debug/program"),
            first.join("target/debug/deps-program"),
        )
        .unwrap();
        fs::create_dir_all(second.join("node_modules")).unwrap();
        fs::hard_link(
            first.join("target/debug/program"),
            second.join("node_modules/linked.bin"),
        )
        .unwrap();
        write_mebibyte(&outside);
        symlink(&outside, first.join("outside-link")).unwrap();
        write_mebibyte(&nested.join("own.bin"));

        let measured = walk_project(&[
            target(&first, &[("target", "cargo-target")]),
            target(&second, &[("node_modules", "node-modules")]),
            target(&nested, &[]),
        ]);

        let [first, second, nested] = measured.as_slice() else {
            panic!("one result for each target: {measured:?}");
        };
        // Two names of one file of 1 MiB in the first worktree, the link
        // out and the nested worktree not counted.
        assert!(
            (MEBIBYTE..MEBIBYTE + SMALL).contains(&first.total_bytes),
            "{first:?}"
        );
        assert_eq!(first.build_outputs[0].0, "target");
        assert!(first.build_outputs[0].2 >= MEBIBYTE, "{first:?}");
        // The second worktree has a third name of the same file.
        assert!(second.total_bytes < SMALL, "{second:?}");
        assert!(second.build_outputs[0].2 < SMALL, "{second:?}");
        assert!(nested.total_bytes >= MEBIBYTE, "{nested:?}");
    }

    #[test]
    fn a_missing_root_measures_nothing() {
        let directory = tempfile::tempdir().unwrap();

        let measured = walk_project(&[target(
            &directory.path().join("gone"),
            &[("target", "cargo-target")],
        )]);

        assert_eq!(measured[0].total_bytes, 0);
        assert_eq!(
            measured[0].build_outputs,
            [("target".to_owned(), "cargo-target", 0)]
        );
    }
}
