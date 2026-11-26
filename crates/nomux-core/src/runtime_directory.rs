//! The runtime directory: the private directory of the user with the
//! socket of the daemon and the directories of the holders.
//!
//! The path of a Unix socket has a limit: 103 bytes on macOS and 107 bytes
//! on Linux, because `sun_path` has 104 and 108 bytes with the closing zero.
//! The runtime directory is `~/.nomux/run`, thus a long home directory makes
//! the paths of the sockets too long. Then nomux opens the runtime directory
//! through a short link in `/tmp`: [`socket_directory`] has the rule.

use std::env;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};

/// The length limit of a socket path, in bytes, on macOS. The limit on
/// Linux is larger, thus the rule uses this limit on each host: a client
/// knows the socket path of a remote host without its operating system.
pub const PORTABLE_SOCKET_PATH_LIMIT: usize = 103;

/// The length limit of a socket path on this host, in bytes.
#[cfg(target_os = "macos")]
pub const SOCKET_PATH_LIMIT: usize = 103;
/// The length limit of a socket path on this host, in bytes.
#[cfg(not(target_os = "macos"))]
pub const SOCKET_PATH_LIMIT: usize = 107;

/// The bytes that the longest socket path has after the runtime directory:
/// `/<session identifier of 8 characters>.pool/sock`, the control socket of
/// a pooled holder.
pub const LONGEST_SOCKET_NAME: usize = 19;

/// The directory of the short links.
const LINK_DIRECTORY: &str = "/tmp";

/// The runtime directory for the home directory `home`: `<home>/.nomux/run`.
#[must_use]
pub fn home_runtime_directory(home: &Path) -> PathBuf {
    home.join(".nomux").join("run")
}

/// The runtime directory of this user, from `HOME`.
///
/// # Errors
///
/// Fails if `HOME` is not set.
pub fn locate() -> io::Result<PathBuf> {
    let home = env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .ok_or_else(|| {
            io::Error::other("the HOME variable is not set, thus the runtime directory has no path")
        })?;
    Ok(home_runtime_directory(Path::new(&home)))
}

/// The path through which nomux opens `runtime_directory` and makes its
/// sockets.
///
/// It is `runtime_directory` if its longest socket path has
/// [`PORTABLE_SOCKET_PATH_LIMIT`] bytes at most. If not, it is the link
/// `/tmp/nomux-<hash>`, where `<hash>` is the FNV-1a hash of 64 bits of the
/// path of `runtime_directory` in hexadecimal digits. The files stay in
/// `runtime_directory`: the link only gives them a short path.
#[must_use]
pub fn socket_directory(runtime_directory: &Path) -> PathBuf {
    let length = runtime_directory.as_os_str().len() + LONGEST_SOCKET_NAME;
    if length <= PORTABLE_SOCKET_PATH_LIMIT {
        return runtime_directory.to_owned();
    }
    let hash = fnv1a(runtime_directory.as_os_str().as_bytes());
    Path::new(LINK_DIRECTORY).join(format!("nomux-{hash:016x}"))
}

/// Gives the path of [`socket_directory`], and makes its link if it has
/// none. The link must point to `runtime_directory` and belong to this
/// user. `/tmp` is open to each user, thus a different file at the path of
/// the link is an error and stays.
///
/// # Errors
///
/// Fails if the link cannot be made, or if the path has a file that is not
/// the link of this user to `runtime_directory`.
pub fn link_socket_directory(runtime_directory: &Path) -> io::Result<PathBuf> {
    let link = socket_directory(runtime_directory);
    if link == runtime_directory {
        return Ok(link);
    }
    match symlink(runtime_directory, &link) {
        Ok(()) => Ok(link),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            check_link(&link, runtime_directory)?;
            Ok(link)
        }
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("the link {} cannot be made: {error}", link.display()),
        )),
    }
}

/// Fails with a message that has the path and the limit if `path` is too
/// long for a socket on this host. A caller checks the path before `bind`.
///
/// # Errors
///
/// Fails if `path` has more than [`SOCKET_PATH_LIMIT`] bytes.
pub fn check_socket_path(path: &Path) -> io::Result<()> {
    check_socket_path_with_limit(path, SOCKET_PATH_LIMIT)
}

fn check_socket_path_with_limit(path: &Path, limit: usize) -> io::Result<()> {
    let length = path.as_os_str().len();
    if length <= limit {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "the socket path {} has {length} bytes, and the limit on this system is {limit} bytes",
            path.display()
        ),
    ))
}

fn check_link(link: &Path, runtime_directory: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(link)?;
    let owner = rustix::process::getuid().as_raw();
    let target = if metadata.file_type().is_symlink() && metadata.uid() == owner {
        Some(fs::read_link(link)?)
    } else {
        None
    };
    if target.as_deref() == Some(runtime_directory) {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{} is not the link of this user to the runtime directory {}. Remove it, or use a \
         shorter home directory",
        link.display(),
        runtime_directory.display()
    )))
}

/// The FNV-1a hash of 64 bits. The value of a path must be the same in each
/// version and on each host, thus the hash of the standard library, which
/// has no fixed algorithm, is not usable.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A home directory of `length` bytes.
    fn home_of(length: usize) -> PathBuf {
        let home = format!("/Users/{}", "a".repeat(length - "/Users/".len()));
        PathBuf::from(home)
    }

    #[test]
    fn the_runtime_directory_of_a_home_of_73_bytes_keeps_its_path() {
        let runtime_directory = home_runtime_directory(&home_of(73));
        assert_eq!(socket_directory(&runtime_directory), runtime_directory);
    }

    #[test]
    fn a_home_of_79_bytes_gets_a_short_link_that_fits_the_limit_of_macos() {
        // Issue #65: with nomux 0.3.0, a home of 79 bytes made the socket of
        // a holder too long for macOS.
        for length in [74, 79, 200] {
            let runtime_directory = home_runtime_directory(&home_of(length));
            let directory = socket_directory(&runtime_directory);
            assert!(directory.starts_with("/tmp"), "{}", directory.display());
            let longest = directory.as_os_str().len() + LONGEST_SOCKET_NAME;
            assert!(longest <= PORTABLE_SOCKET_PATH_LIMIT, "{length}: {longest}");
        }
    }

    #[test]
    fn two_runtime_directories_get_two_links() {
        let first = socket_directory(&home_runtime_directory(&home_of(90)));
        let second = socket_directory(&home_runtime_directory(&home_of(91)));
        assert_ne!(first, second);
    }

    #[test]
    fn a_socket_path_over_the_limit_of_macos_fails_with_the_path_and_the_limit() {
        let path = PathBuf::from(format!("/{}", "a".repeat(103)));
        assert!(check_socket_path_with_limit(&path, 107).is_ok());
        let error = check_socket_path_with_limit(&path, 103).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "the socket path {} has 104 bytes, and the limit on this system is 103 bytes",
                path.display()
            )
        );
    }

    #[test]
    fn a_file_at_the_path_of_the_link_is_an_error_and_stays() {
        let home = tempfile::Builder::new()
            .prefix(&"a".repeat(100))
            .tempdir_in("/tmp")
            .unwrap();
        let runtime_directory = home_runtime_directory(home.path());
        let link = socket_directory(&runtime_directory);
        fs::write(&link, "").unwrap();
        let error = link_socket_directory(&runtime_directory).unwrap_err();
        let file_stays = fs::symlink_metadata(&link).unwrap().is_file();
        fs::remove_file(&link).unwrap();
        assert!(error.to_string().contains("is not the link"), "{error}");
        assert!(file_stays);
    }
}
