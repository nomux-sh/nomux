//! The operating system and the processor of a remote host.

use crate::ClientError;

/// The release targets: the name of each release archive ends with one.
pub(crate) const RELEASE_TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// A host that has a release target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemotePlatform {
    /// The release target: the name of the archive of a release has it.
    pub(crate) target: &'static str,
}

impl RemotePlatform {
    /// Finds the release target from the output of `uname -s` and
    /// `uname -m`. A Linux host gets the static binaries, which do not use
    /// the C library of the host.
    pub(crate) fn from_uname(system: &str, machine: &str) -> Result<Self, ClientError> {
        let target = match (system, machine) {
            ("Linux", "x86_64" | "amd64") => RELEASE_TARGETS[0],
            ("Linux", "aarch64" | "arm64") => RELEASE_TARGETS[1],
            ("Darwin", "x86_64") => RELEASE_TARGETS[2],
            ("Darwin", "arm64" | "aarch64") => RELEASE_TARGETS[3],
            _ => {
                return Err(ClientError::Failed(format!(
                    "nomux has no binaries for the host: the operating system is `{system}` and \
                     the processor is `{machine}`. Supported: Linux and macOS on x86_64 and \
                     aarch64"
                )));
            }
        };
        Ok(Self { target })
    }

    /// The release target of this computer.
    pub(crate) fn of_this_computer() -> Result<Self, ClientError> {
        let system = match std::env::consts::OS {
            "linux" => "Linux",
            "macos" => "Darwin",
            other => other,
        };
        Self::from_uname(system, std::env::consts::ARCH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_release_target_has_its_uname_output() {
        let target = |system, machine| RemotePlatform::from_uname(system, machine).unwrap().target;
        assert_eq!(target("Linux", "x86_64"), "x86_64-unknown-linux-musl");
        assert_eq!(target("Linux", "aarch64"), "aarch64-unknown-linux-musl");
        assert_eq!(target("Darwin", "x86_64"), "x86_64-apple-darwin");
        assert_eq!(target("Darwin", "arm64"), "aarch64-apple-darwin");
    }

    #[test]
    fn a_host_without_a_release_target_is_refused_with_its_uname_output() {
        let message = RemotePlatform::from_uname("FreeBSD", "amd64")
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("FreeBSD") && message.contains("amd64"),
            "{message}"
        );
        assert!(RemotePlatform::from_uname("Linux", "armv7l").is_err());
    }
}
