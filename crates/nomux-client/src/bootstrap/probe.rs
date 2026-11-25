//! The first command on a remote host: what the host is and whether it has
//! the binaries.

use nomux_transport::ssh::SshConnection;

use super::{remote_shell, script_failed};
use crate::ClientError;

/// The line before the answers. The login shell of the user can write
/// text before it, for example from a start file.
const MARKER: &str = "nomux-probe";

/// What the probe found on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Probe {
    /// The output of `uname -s`.
    pub(crate) system: String,
    /// The output of `uname -m`.
    pub(crate) machine: String,
    /// The home directory of the user on the host.
    pub(crate) home: String,
    /// `true` if `~/.nomux/bin/<label>` has the three binaries.
    pub(crate) installed: bool,
}

/// Asks the host for its operating system, its processor, the home
/// directory of the user and the state of the installation `label`.
pub(crate) async fn probe(connection: &SshConnection, label: &str) -> Result<Probe, ClientError> {
    let script = format!(
        "echo {MARKER}; uname -s; uname -m; printf \"%s\\n\" \"$HOME\"; \
         d=\"$HOME/.nomux/bin/{label}\"; \
         if [ -x \"$d/nomux\" ] && [ -x \"$d/nomuxd\" ] && [ -x \"$d/nomux-holder\" ]; \
         then echo installed; else echo missing; fi"
    );
    let output = connection.run(&remote_shell(&script), &[]).await?;
    if !output.succeeded() {
        return Err(script_failed("the probe of the host", &output));
    }
    parse(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
        ClientError::Failed(
            "the host did not answer the probe: `uname` and a POSIX shell (`sh`) are necessary \
             on the host"
                .to_owned(),
        )
    })
}

fn parse(output: &str) -> Option<Probe> {
    let mut lines = output.lines().skip_while(|line| *line != MARKER).skip(1);
    let mut answer = || lines.next().filter(|line| !line.is_empty());
    Some(Probe {
        system: answer()?.to_owned(),
        machine: answer()?.to_owned(),
        home: answer()?.to_owned(),
        installed: match answer()? {
            "installed" => true,
            "missing" => false,
            _ => return None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_of_a_start_file_before_the_answers_is_ignored() {
        let probe =
            parse("Welcome to vps!\nLinux\nnomux-probe\nLinux\nx86_64\n/home/deploy\nmissing\n");

        assert_eq!(
            probe,
            Some(Probe {
                system: "Linux".to_owned(),
                machine: "x86_64".to_owned(),
                home: "/home/deploy".to_owned(),
                installed: false,
            })
        );
    }

    #[test]
    fn an_answer_that_is_not_complete_is_no_probe() {
        assert_eq!(parse("nomux-probe\nLinux\nx86_64\n"), None);
        assert_eq!(parse("Linux\nx86_64\n/home/deploy\ninstalled\n"), None);
        assert_eq!(parse("nomux-probe\nLinux\nx86_64\n\ninstalled\n"), None);
    }
}
