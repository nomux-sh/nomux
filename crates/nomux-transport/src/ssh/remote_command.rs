//! A command on the host, on its own channel of the SSH connection.

use russh::ChannelMsg;

use super::{SshConnection, SshError};

/// The largest output that the client keeps from one command.
const MAX_OUTPUT: usize = 64 * 1024 * 1024;
/// The channel number of the standard error in the SSH protocol.
const STANDARD_ERROR: u32 = 1;

/// The result of a command that ran on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The exit status. `None` if a signal ended the command.
    pub exit_status: Option<u32>,
    /// The standard output.
    pub stdout: Vec<u8>,
    /// The standard error.
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    /// Returns `true` if the exit status is 0.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.exit_status == Some(0)
    }
}

impl SshConnection {
    /// Runs `command` with the login shell of the user on the host, gives
    /// it `input` as its standard input and waits for its end. The command
    /// has no terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails, if the SSH server refuses
    /// the command, or if the output is too long. A command that ends with
    /// a status that is not 0 is not an error of this function.
    pub async fn run(&self, command: &str, input: &[u8]) -> Result<CommandOutput, SshError> {
        let mut channel = self.session().channel_open_session().await?;
        channel.exec(true, command).await?;
        if !input.is_empty() {
            channel.data(input).await?;
        }
        channel.eof().await?;

        let mut output = CommandOutput {
            exit_status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => output.stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, ext } if ext == STANDARD_ERROR => {
                    output.stderr.extend_from_slice(&data);
                }
                ChannelMsg::ExitStatus { exit_status } => output.exit_status = Some(exit_status),
                ChannelMsg::Failure => return Err(SshError::CommandRefused),
                _ => {}
            }
            if output.stdout.len() + output.stderr.len() > MAX_OUTPUT {
                return Err(SshError::Protocol(
                    "a command on the host wrote more output than nomux accepts".to_owned(),
                ));
            }
        }
        Ok(output)
    }
}
