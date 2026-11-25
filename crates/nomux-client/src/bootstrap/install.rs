//! The installation of the binaries on a remote host: upload, checksum,
//! and one rename.

use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use nomux_transport::ssh::SshConnection;

use super::binaries::{BINARY_NAMES, Binaries, sha256_hex};
use super::{VERSION, remote_shell, script_failed};
use crate::ClientError;

/// The answer of the host if it has no program that makes a SHA-256
/// checksum.
const NO_CHECKSUM_PROGRAM: &str = "no-sha256-program";

/// Installs `binaries` into `~/.nomux/bin/<label>` on the host.
///
/// The files go into a directory with a temporary name. The client then
/// compares the SHA-256 checksum of each file on the host with its own,
/// runs the new `nomuxd` one time, and renames the directory. Thus
/// `~/.nomux/bin/<label>` exists only with three complete binaries that run
/// on the host. The directories and the files are private to the user.
pub(crate) async fn install(
    connection: &SshConnection,
    label: &str,
    binaries: &Binaries,
) -> Result<(), ClientError> {
    let staging = format!(".{label}.{}", unique_suffix());
    let installed = install_through(connection, label, &staging, binaries).await;
    if installed.is_err() {
        // The directory of an installation that failed has no use.
        let remove = format!("rm -rf \"$HOME/.nomux/bin/{staging}\"");
        let _ = connection.run(&remote_shell(&remove), &[]).await;
    }
    installed
}

async fn install_through(
    connection: &SshConnection,
    label: &str,
    staging: &str,
    binaries: &Binaries,
) -> Result<(), ClientError> {
    let run = async |what: &str, script: String, input: &[u8]| {
        let output = connection.run(&remote_shell(&script), input).await?;
        if output.succeeded() {
            Ok(output.stdout)
        } else {
            Err(script_failed(what, &output))
        }
    };
    let directory = format!("\"$HOME/.nomux/bin/{staging}\"");

    // `cat` is sufficient for an upload: the host needs no `scp` and no
    // `sftp`.
    for (name, bytes) in binaries.files() {
        let script = format!("umask 077 && mkdir -p {directory} && cat > {directory}/{name}");
        run("the upload of a binary", script, bytes).await?;
    }

    let names = BINARY_NAMES.join(" ");
    let checksum_script = format!(
        "cd {directory} && chmod 700 . {names} && \
         if command -v sha256sum >/dev/null 2>&1; then sha256sum {names}; \
         elif command -v shasum >/dev/null 2>&1; then shasum -a 256 {names}; \
         else echo {NO_CHECKSUM_PROGRAM}; fi"
    );
    let remote_checksums = run("the checksum of the binaries", checksum_script, &[]).await?;
    let remote_checksums = String::from_utf8_lossy(&remote_checksums).into_owned();
    for (name, bytes) in binaries.files() {
        let on_host = if remote_checksums.trim() == NO_CHECKSUM_PROGRAM {
            // A host without a checksum program sends the file back.
            let copy = run(
                "the read of a binary",
                format!("cat {directory}/{name}"),
                &[],
            )
            .await?;
            sha256_hex(&copy)
        } else {
            checksum_of(&remote_checksums, name).unwrap_or_default()
        };
        if on_host != sha256_hex(bytes) {
            return Err(ClientError::Failed(format!(
                "the SHA-256 checksum of {name} on the host is not the checksum of the file that \
                 the client sent. Nothing was installed"
            )));
        }
    }

    let version = run(
        "the first run of the new nomuxd",
        format!("{directory}/nomuxd --version"),
        &[],
    )
    .await?;
    if String::from_utf8_lossy(&version).trim() != format!("nomuxd {VERSION}") {
        return Err(ClientError::Failed(format!(
            "the binaries are not nomux {VERSION} for the host: `nomuxd --version` printed `{}`",
            String::from_utf8_lossy(&version).trim()
        )));
    }

    // The rename is the one step that makes the installation exist.
    let commit =
        format!("cd \"$HOME/.nomux/bin\" && rm -rf \"{label}\" && mv \"{staging}\" \"{label}\"");
    run("the rename of the installation directory", commit, &[]).await?;
    Ok(())
}

/// The checksum of `name` in the output of `sha256sum` or `shasum`:
/// `<checksum>  <name>` on each line.
fn checksum_of(output: &str, name: &str) -> Option<String> {
    output
        .lines()
        .filter_map(|line| line.split_once(char::is_whitespace))
        .find(|(_, file)| file.trim_start_matches([' ', '*']) == name)
        .map(|(checksum, _)| checksum.to_ascii_lowercase())
}

/// A name part that two installations at the same time do not share.
fn unique_suffix() -> String {
    let nanoseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.subsec_nanos());
    format!("{:x}-{nanoseconds:x}", process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksum_of_one_file_is_found_in_the_two_output_forms() {
        let gnu = "aa11  nomux\nbb22  nomuxd\ncc33  nomux-holder\n";
        assert_eq!(checksum_of(gnu, "nomuxd").as_deref(), Some("bb22"));
        assert_eq!(checksum_of(gnu, "nomux").as_deref(), Some("aa11"));
        // The binary mode of `sha256sum` has `*` before the name.
        assert_eq!(
            checksum_of("DD44 *nomux\n", "nomux").as_deref(),
            Some("dd44")
        );
        assert_eq!(checksum_of(gnu, "missing"), None);
    }
}
