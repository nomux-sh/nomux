//! The workspace tree on disk: the file `workspace.json` in the state
//! directory of the daemon. `docs/workspace.md` has the format.
//!
//! A write replaces the file in one step: the tree goes to a new file, the
//! new file goes to the disk, and a rename puts it in the place of the old
//! one. A file that the daemon cannot use stays next to it with a new name.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use nomux_core::workspace_tree::{Node, NodeKind, WorkspaceTree};
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use super::limits::{self, MAX_NODES};
use crate::clock;
use crate::session_store::small_file;

const FILE: &str = "workspace.json";
const NEW_FILE: &str = "workspace.json.new";
/// The version of the format. A daemon reads only the versions that it
/// knows.
const VERSION: u32 = 1;
/// Larger than the largest tree, with each character of each string as an
/// escape sequence.
const MAX_LENGTH: u64 = 8 * 1024 * 1024;
const OWNER_READ_WRITE: u32 = 0o600;

/// The tree and its revision, as the file has them.
#[derive(Debug, Default)]
pub(super) struct Loaded {
    pub(super) tree: WorkspaceTree,
    pub(super) revision: u64,
}

#[derive(Serialize, Deserialize)]
struct StoredTree {
    version: u32,
    revision: u64,
    /// In the sequence of a depth-first walk.
    nodes: Vec<StoredNode>,
}

/// Only the version: a file of a later version can have a different
/// form.
#[derive(Deserialize)]
struct StoredVersion {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct StoredNode {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    name: String,
    /// Only a Project has a directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directory: Option<String>,
}

pub(super) struct TreeFile {
    directory: PathBuf,
}

impl TreeFile {
    /// The file in the state directory `directory`.
    pub(super) fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
        }
    }

    /// The tree of the file. Without a file, the tree is empty and has the
    /// revision 0. A file that the daemon cannot use goes aside, and the
    /// tree is empty.
    pub(super) fn load(&self) -> Loaded {
        let path = self.directory.join(FILE);
        match fs::exists(&path) {
            Ok(false) => return Loaded::default(),
            Ok(true) => {}
            Err(reason) => return self.set_aside(&path, &reason.to_string()),
        }
        match read(&path) {
            Ok(loaded) => loaded,
            Err(reason) => self.set_aside(&path, &reason),
        }
    }

    /// Replaces the file with `tree` and `revision`.
    pub(super) fn write(&self, tree: &WorkspaceTree, revision: u64) -> io::Result<()> {
        let stored = StoredTree {
            version: VERSION,
            revision,
            nodes: tree.nodes().into_iter().map(StoredNode::from).collect(),
        };
        let bytes = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
        fs::create_dir_all(&self.directory)?;
        let new_file = self.directory.join(NEW_FILE);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(OWNER_READ_WRITE)
            .open(&new_file)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&new_file, self.directory.join(FILE))?;
        // The rename is on the disk only when the directory is.
        File::open(&self.directory)?.sync_all()
    }

    /// Keeps the file at `path` with a new name and gives an empty tree.
    fn set_aside(&self, path: &Path, reason: &str) -> Loaded {
        let now = clock::now().0;
        let aside = self.directory.join(format!("{FILE}.{now}.damaged"));
        match fs::rename(path, &aside) {
            Ok(()) => warn!(
                file = %path.display(),
                kept = %aside.display(),
                reason,
                "the daemon cannot use the workspace tree file: it keeps the file aside and \
                 starts with an empty tree"
            ),
            Err(rename_error) => error!(
                file = %path.display(),
                reason,
                %rename_error,
                "the daemon cannot use the workspace tree file and cannot keep it aside: it \
                 starts with an empty tree, and the next change replaces the file"
            ),
        }
        Loaded {
            tree: WorkspaceTree::default(),
            // The revision of the lost tree is not known. The time in
            // milliseconds is larger than each revision that changes can
            // reach, thus the revision does not decrease.
            revision: now,
        }
    }
}

/// Why the file at `path` cannot be used: a text for a person.
fn read(path: &Path) -> Result<Loaded, String> {
    let bytes = small_file::read(path, MAX_LENGTH).map_err(|error| error.to_string())?;
    let StoredVersion { version } =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if version != VERSION {
        return Err(format!(
            "the file has the version {version}, and this daemon reads only the version \
             {VERSION}"
        ));
    }
    let stored: StoredTree = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if stored.nodes.len() > MAX_NODES {
        return Err(format!(
            "the file has {} nodes: {MAX_NODES} is the limit",
            stored.nodes.len()
        ));
    }
    let nodes = stored
        .nodes
        .into_iter()
        .map(Node::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let tree = WorkspaceTree::from_nodes(nodes).map_err(|error| error.to_string())?;
    Ok(Loaded {
        tree,
        revision: stored.revision,
    })
}

impl From<&Node> for StoredNode {
    fn from(node: &Node) -> Self {
        Self {
            id: node.id.clone(),
            parent_id: node.parent_id.clone(),
            name: node.name.clone(),
            directory: node
                .directory()
                .map(|directory| directory.to_string_lossy().into_owned()),
        }
    }
}

impl TryFrom<StoredNode> for Node {
    type Error = String;

    fn try_from(stored: StoredNode) -> Result<Self, String> {
        if !limits::is_node_identifier(&stored.id) {
            return Err(format!("{:?} is not a node identifier", stored.id));
        }
        limits::check_name(&stored.name)
            .map_err(|reason| format!("node {}: {reason}", stored.id))?;
        let kind = match stored.directory {
            None => NodeKind::Group,
            Some(directory) if Path::new(&directory).is_absolute() => {
                limits::check_directory(&directory)?;
                NodeKind::Project {
                    directory: directory.into(),
                }
            }
            Some(directory) => {
                return Err(format!(
                    "node {}: the directory {directory} is not absolute",
                    stored.id
                ));
            }
        };
        Ok(Self {
            id: stored.id,
            parent_id: stored.parent_id,
            name: stored.name,
            kind,
        })
    }
}
