//! The entries of the release archive of an extension: a `.tar.gz` file
//! with one package directory.
//!
//! The reader keeps each entry in memory and writes nothing. It refuses an
//! entry that is not a regular file or a directory (a link, a device, a
//! FIFO, an extended header), a path that is absolute or that leaves the
//! package directory, and an archive above the limits.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;

/// The largest number of entries of an archive.
pub(crate) const MAX_ENTRIES: usize = 256;
/// The largest sum of the sizes of the files of an archive: 256 MiB.
pub(crate) const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;

const BLOCK: usize = 512;
const NAME: std::ops::Range<usize> = 0..100;
const MODE: std::ops::Range<usize> = 100..108;
const SIZE: std::ops::Range<usize> = 124..136;
const CHECKSUM: std::ops::Range<usize> = 148..156;
const TYPE: usize = 156;
const MAGIC: std::ops::Range<usize> = 257..265;
const PREFIX: std::ops::Range<usize> = 345..500;
/// The magic and the version of a POSIX `ustar` header, which can have a
/// prefix of the path. A GNU header has other fields at that place.
const POSIX_MAGIC: &[u8] = b"ustar\x0000";
const EXECUTABLE: u32 = 0o111;

/// One file or directory of the package, with its path in the package
/// directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PackageEntry {
    pub(crate) path: PathBuf,
    pub(crate) kind: EntryKind,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Directory,
    File { executable: bool, content: Vec<u8> },
}

/// The package directory of an archive and its entries.
#[derive(Debug)]
pub(crate) struct ArchiveEntries {
    /// The name of the package directory.
    pub(crate) package: String,
    pub(crate) entries: Vec<PackageEntry>,
}

/// Reads the entries of the archive `bytes`.
///
/// # Errors
///
/// Returns the reason if the archive is not a `.tar.gz` file, has an entry
/// that the rules refuse, or is above a limit.
pub(crate) fn read(bytes: &[u8]) -> Result<ArchiveEntries, String> {
    let mut decoder = GzDecoder::new(bytes);
    let mut package: Option<String> = None;
    let mut entries = Vec::new();
    let mut unpacked: u64 = 0;
    loop {
        let mut header = [0; BLOCK];
        read_exact(&mut decoder, &mut header)?;
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        if entries.len() == MAX_ENTRIES {
            return Err(format!("the archive has more than {MAX_ENTRIES} entries"));
        }
        check_header_checksum(&header)?;
        let path = header_path(&header)?;
        let size = octal(&header[SIZE]).ok_or_else(|| format!("the entry {path} has no size"))?;
        let kind = match header[TYPE] {
            b'0' | b'\0' => {
                unpacked += size;
                if unpacked > MAX_UNPACKED_BYTES {
                    return Err(format!(
                        "the files of the archive have more than {} MiB; the entry {path} has \
                         {size} bytes",
                        MAX_UNPACKED_BYTES / 1024 / 1024
                    ));
                }
                let mut content = vec![0; usize::try_from(size).map_err(|_| "too large")?];
                read_exact(&mut decoder, &mut content)?;
                skip_padding(&mut decoder, content.len())?;
                let mode = octal(&header[MODE]).unwrap_or(0);
                EntryKind::File {
                    executable: u32::try_from(mode).unwrap_or(0) & EXECUTABLE != 0,
                    content,
                }
            }
            b'5' => EntryKind::Directory,
            other => {
                return Err(format!(
                    "the entry {path} is {}, and an archive can have only regular files and \
                     directories",
                    type_name(other)
                ));
            }
        };
        let (top, inner) = split_package(&path)?;
        match &package {
            None => package = Some(top.clone()),
            Some(known) if *known == top => {}
            Some(known) => {
                return Err(format!(
                    "the entry {path} is not in the package directory {known}"
                ));
            }
        }
        match (inner, kind) {
            (None, EntryKind::Directory) => {}
            (None, EntryKind::File { .. }) => {
                return Err(format!(
                    "the entry {path} is a file, not a package directory"
                ));
            }
            (Some(path), kind) => entries.push(PackageEntry { path, kind }),
        }
    }
    let package = package.ok_or("the archive is empty")?;
    Ok(ArchiveEntries { package, entries })
}

fn read_exact(decoder: &mut impl Read, buffer: &mut [u8]) -> Result<(), String> {
    decoder
        .read_exact(buffer)
        .map_err(|error| format!("the archive is not a complete .tar.gz file: {error}"))
}

fn skip_padding(decoder: &mut impl Read, length: usize) -> Result<(), String> {
    let padding = (BLOCK - length % BLOCK) % BLOCK;
    read_exact(decoder, &mut [0; BLOCK][..padding])
}

fn check_header_checksum(header: &[u8; BLOCK]) -> Result<(), String> {
    let expected = octal(&header[CHECKSUM]);
    let actual: u64 = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if CHECKSUM.contains(&index) {
                u64::from(b' ')
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    if expected == Some(actual) {
        Ok(())
    } else {
        Err("the archive is not a tar archive: a header has a wrong checksum".to_owned())
    }
}

/// The path of the entry of `header`, with the prefix of a POSIX header.
fn header_path(header: &[u8; BLOCK]) -> Result<String, String> {
    let name = text(&header[NAME])?;
    if header[MAGIC] != *POSIX_MAGIC {
        return Ok(name);
    }
    match text(&header[PREFIX])? {
        prefix if prefix.is_empty() => Ok(name),
        prefix => Ok(format!("{prefix}/{name}")),
    }
}

fn text(field: &[u8]) -> Result<String, String> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    String::from_utf8(field[..end].to_vec())
        .map_err(|_| "the archive has a path that is not UTF-8".to_owned())
}

/// A number in octal digits, with spaces or a zero at the end. `None` for
/// another form, also for the binary form of a large number.
fn octal(field: &[u8]) -> Option<u64> {
    let digits = std::str::from_utf8(field).ok()?;
    let digits = digits.trim_matches(|character| character == ' ' || character == '\0');
    u64::from_str_radix(digits, 8).ok()
}

/// The package directory of `path` and the path in it. `None` for the
/// package directory itself.
fn split_package(path: &str) -> Result<(String, Option<PathBuf>), String> {
    let leaves = || {
        format!("the entry {path} is not a plain relative path: it can leave the package directory")
    };
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(leaves)?.to_owned()),
            _ => return Err(leaves()),
        }
    }
    if let Some(part) = parts
        .iter()
        .find(|part| !part.bytes().all(is_path_character))
    {
        return Err(format!(
            "the entry {path} has the name {part:?}: a name in a package has only letters,              digits and the characters . _ + -"
        ));
    }
    if path.contains("//") || path.split('/').any(|part| part == ".") {
        return Err(leaves());
    }
    let (top, inner) = parts.split_first().ok_or_else(leaves)?;
    let inner: PathBuf = inner.iter().collect();
    Ok((
        top.clone(),
        (!inner.as_os_str().is_empty()).then_some(inner),
    ))
}

/// A byte of a name in a package. The rule keeps each path safe in a
/// command of a POSIX shell in double quotes, for the upload to a remote
/// host.
fn is_path_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
}

fn type_name(entry_type: u8) -> String {
    match entry_type {
        b'1' => "a hard link".to_owned(),
        b'2' => "a symbolic link".to_owned(),
        b'3' => "a character device".to_owned(),
        b'4' => "a block device".to_owned(),
        b'6' => "a FIFO".to_owned(),
        b'x' | b'g' => "an extended header (pax)".to_owned(),
        b'L' | b'K' => "a long name header (GNU)".to_owned(),
        other => format!("an entry of the type {:?}", char::from(other)),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    const PACKAGE: &str = "nomux-test-1.0.0-x86_64-unknown-linux-musl";

    /// One header of a GNU archive, as GNU `tar` writes it.
    fn header(path: &str, entry_type: u8, size: u64) -> [u8; BLOCK] {
        let mut header = [0; BLOCK];
        header[..path.len()].copy_from_slice(path.as_bytes());
        header[MODE].copy_from_slice(b"0000755\0");
        header[SIZE].copy_from_slice(format!("{size:011o}\0").as_bytes());
        header[TYPE] = entry_type;
        header[MAGIC].copy_from_slice(b"ustar  \0");
        header[CHECKSUM].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        header[CHECKSUM].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        header
    }

    /// A `.tar.gz` archive with `entries`: a path, a type and a content.
    fn archive(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (path, entry_type, content) in entries {
            tar.extend_from_slice(&header(path, *entry_type, content.len() as u64));
            tar.extend_from_slice(content);
            tar.resize(tar.len().div_ceil(BLOCK) * BLOCK, 0);
        }
        tar.extend_from_slice(&[0; 2 * BLOCK]);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap()
    }

    fn refusal(entries: &[(&str, u8, &[u8])]) -> String {
        read(&archive(entries)).unwrap_err()
    }

    #[test]
    fn a_package_has_its_directories_and_its_files() {
        let package = read(&archive(&[
            (&format!("{PACKAGE}/"), b'5', b""),
            (&format!("{PACKAGE}/extension.toml"), b'0', b"[extension]\n"),
            (&format!("{PACKAGE}/bin/"), b'5', b""),
            (&format!("{PACKAGE}/bin/nomux-test"), b'0', b"program"),
        ]))
        .unwrap();

        assert_eq!(package.package, PACKAGE);
        assert_eq!(
            package.entries,
            [
                PackageEntry {
                    path: "extension.toml".into(),
                    kind: EntryKind::File {
                        executable: true,
                        content: b"[extension]\n".to_vec()
                    }
                },
                PackageEntry {
                    path: "bin".into(),
                    kind: EntryKind::Directory
                },
                PackageEntry {
                    path: "bin/nomux-test".into(),
                    kind: EntryKind::File {
                        executable: true,
                        content: b"program".to_vec()
                    }
                },
            ]
        );
    }

    #[test]
    fn a_path_that_can_leave_the_package_directory_is_refused() {
        for path in [
            format!("{PACKAGE}/../evil"),
            "/etc/evil".to_owned(),
            "../evil".to_owned(),
            format!("{PACKAGE}/./bin"),
            format!("{PACKAGE}//bin"),
        ] {
            let message = refusal(&[(&path, b'0', b"x")]);
            assert!(
                message.contains("can leave the package directory"),
                "{path}: {message}"
            );
        }
        let message = refusal(&[
            (&format!("{PACKAGE}/a"), b'0', b"x"),
            ("other/b", b'0', b"x"),
        ]);
        assert!(
            message.contains("is not in the package directory"),
            "{message}"
        );
        let message = refusal(&[(&format!("{PACKAGE}/a\"$(x)"), b'0', b"x")]);
        assert!(
            message.contains("a name in a package has only"),
            "{message}"
        );
    }

    #[test]
    fn an_entry_that_is_not_a_file_or_a_directory_is_refused() {
        for (entry_type, name) in [
            (b'2', "a symbolic link"),
            (b'1', "a hard link"),
            (b'3', "a character device"),
            (b'4', "a block device"),
            (b'6', "a FIFO"),
            (b'x', "an extended header (pax)"),
            (b'L', "a long name header (GNU)"),
        ] {
            let message = refusal(&[(&format!("{PACKAGE}/entry"), entry_type, b"")]);
            assert!(message.contains(&format!("is {name}")), "{message}");
        }
    }

    #[test]
    fn an_archive_above_a_limit_is_refused_before_its_content_is_read() {
        // Only the header: the reader stops at the size.
        let mut tar = header(&format!("{PACKAGE}/large"), b'0', MAX_UNPACKED_BYTES + 1).to_vec();
        tar.extend_from_slice(&[0; 2 * BLOCK]);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&tar).unwrap();
        let message = read(&encoder.finish().unwrap()).unwrap_err();
        assert!(message.contains("have more than 256 MiB"), "{message}");

        let paths: Vec<String> = (0..=MAX_ENTRIES)
            .map(|index| format!("{PACKAGE}/{index}"))
            .collect();
        let entries: Vec<(&str, u8, &[u8])> = paths
            .iter()
            .map(|path| (path.as_str(), b'0', &b""[..]))
            .collect();
        assert!(refusal(&entries).contains("more than 256 entries"));
    }

    #[test]
    fn a_file_that_is_not_a_tar_gz_archive_is_refused() {
        assert!(
            read(b"not gzip")
                .unwrap_err()
                .contains("not a complete .tar.gz")
        );
        let mut tar = header(&format!("{PACKAGE}/a"), b'0', 1).to_vec();
        tar[0] ^= 1;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&tar).unwrap();
        assert!(
            read(&encoder.finish().unwrap())
                .unwrap_err()
                .contains("wrong checksum")
        );
    }
}
