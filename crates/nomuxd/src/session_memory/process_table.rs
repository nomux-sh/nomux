//! The processes of the host at one moment, with the parent of each one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;

use nomux_holder_protocol::discovery::ProcessIdentity;

use super::process_list;

struct Process {
    start_time: u64,
    parent: u32,
}

/// One process below a root, with the process that started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Descendant {
    pub(super) identity: ProcessIdentity,
    pub(super) parent: u32,
}

/// A copy of the process table. The operating system gives one process at a
/// time, thus the entries are not from the same instant.
pub(super) struct ProcessTable {
    processes: HashMap<u32, Process>,
    children: HashMap<u32, Vec<u32>>,
}

impl ProcessTable {
    /// Reads each process that this user can see.
    pub(super) fn read() -> io::Result<Self> {
        let mut processes = Vec::new();
        for pid in process_list::pids()? {
            // No record: the process ended after the list was made.
            if let Some(process) = ProcessIdentity::with_parent_of(pid)? {
                processes.push(process);
            }
        }
        Ok(Self::of(processes))
    }

    fn of(processes: impl IntoIterator<Item = (ProcessIdentity, u32)>) -> Self {
        let mut table = Self {
            processes: HashMap::new(),
            children: HashMap::new(),
        };
        for (identity, parent) in processes {
            let process = Process {
                start_time: identity.start_time,
                parent,
            };
            table.processes.insert(identity.pid, process);
            table.children.entry(parent).or_default().push(identity.pid);
        }
        table
    }

    /// `true` if the table has the process `identity`, and not a later
    /// process with the same process ID.
    pub(super) fn has(&self, identity: ProcessIdentity) -> bool {
        self.processes
            .get(&identity.pid)
            .is_some_and(|process| process.start_time == identity.start_time)
    }

    /// Each process that descends from `root`, without `root`. A parent is
    /// before its children.
    ///
    /// A process that started before its parent is not a child of that
    /// parent: its real parent ended, and a new process got the process ID.
    pub(super) fn descendants(&self, root: u32) -> Vec<Descendant> {
        let mut found = Vec::new();
        let mut seen = HashSet::from([root]);
        let mut parents = VecDeque::from([root]);
        while let Some(parent) = parents.pop_front() {
            let Some(parent_process) = self.processes.get(&parent) else {
                continue;
            };
            for &pid in self.children.get(&parent).into_iter().flatten() {
                let child = &self.processes[&pid];
                if child.start_time < parent_process.start_time || !seen.insert(pid) {
                    continue;
                }
                found.push(Descendant {
                    identity: ProcessIdentity {
                        pid,
                        start_time: child.start_time,
                    },
                    parent: child.parent,
                });
                parents.push_back(pid);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    fn table(processes: &[(u32, u64, u32)]) -> ProcessTable {
        ProcessTable::of(processes.iter().map(|&(pid, start_time, parent)| {
            let identity = ProcessIdentity { pid, start_time };
            (identity, parent)
        }))
    }

    fn pids(descendants: &[Descendant]) -> Vec<u32> {
        let mut pids: Vec<u32> = descendants
            .iter()
            .map(|process| process.identity.pid)
            .collect();
        pids.sort_unstable();
        pids
    }

    #[test]
    fn the_descendants_are_the_full_tree_below_the_root_and_no_other_process() {
        let table = table(&[
            (1, 0, 0),
            (10, 5, 1),
            (11, 6, 10),
            (12, 7, 11),
            (13, 7, 10),
            // A second child of process 1, with a tree of its own.
            (20, 5, 1),
            (21, 6, 20),
        ]);

        assert_eq!(pids(&table.descendants(10)), [11, 12, 13]);
        assert_eq!(pids(&table.descendants(12)), [] as [u32; 0]);
        // No process has the ID of the root.
        assert_eq!(pids(&table.descendants(99)), [] as [u32; 0]);
        let descendants = table.descendants(10);
        let position = |pid| {
            descendants
                .iter()
                .position(|process| process.identity.pid == pid)
        };
        assert!(position(11) < position(12), "a parent is before its child");
    }

    #[test]
    fn a_process_that_is_older_than_its_parent_is_not_a_descendant() {
        // Process 30 was the child of an earlier process 11. That process
        // ended, and the new process 11 of the tree got its process ID.
        let table = table(&[(10, 5, 1), (11, 50, 10), (30, 20, 11), (31, 60, 30)]);

        assert_eq!(pids(&table.descendants(10)), [11]);
    }

    #[test]
    fn the_table_of_the_host_has_this_process_and_its_child() {
        let mut child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let this_process = ProcessIdentity::current().unwrap();

        let table = ProcessTable::read().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();

        assert!(table.has(this_process));
        let descendants = table.descendants(this_process.pid);
        let found = descendants
            .iter()
            .find(|process| process.identity.pid == child.id())
            .unwrap_or_else(|| panic!("the child is not in {descendants:?}"));
        assert_eq!(found.parent, this_process.pid);
    }
}
