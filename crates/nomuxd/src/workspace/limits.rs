//! The limits of the workspace tree. The response to `Workspace.Tree` goes
//! into one envelope only with them: `spec/workspace.md` has the
//! calculation.

/// The largest number of nodes in the tree.
pub(super) const MAX_NODES: usize = 800;
/// The largest number of bytes of a name.
const MAX_NAME_BYTES: usize = 128;
/// The largest number of characters of a node identifier.
const MAX_IDENTIFIER_LENGTH: usize = 32;
/// The largest number of bytes of the directory of a Project, after the
/// symbolic links are resolved: `PATH_MAX` of macOS.
const MAX_DIRECTORY_BYTES: usize = 1024;

/// Why `name` is not the name of a node: 1 to 128 bytes of UTF-8 without a
/// control character.
pub(super) fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the name is empty".to_owned());
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(format!(
            "the name has {} bytes: {MAX_NAME_BYTES} bytes is the limit",
            name.len()
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("the name has a control character".to_owned());
    }
    Ok(())
}

/// Why `directory` is too long for the directory of a Project.
pub(super) fn check_directory(directory: &str) -> Result<(), String> {
    if directory.len() > MAX_DIRECTORY_BYTES {
        return Err(format!(
            "the directory {directory} has {} bytes: {MAX_DIRECTORY_BYTES} bytes is the limit",
            directory.len()
        ));
    }
    Ok(())
}

/// `true` if `id` is a node identifier: 1 to 32 characters from `A-Z`,
/// `a-z`, `0-9`, `-` and `_`.
pub(super) fn is_node_identifier(id: &str) -> bool {
    (1..=MAX_IDENTIFIER_LENGTH).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}
