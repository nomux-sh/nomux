//! The start of a pooled shell that changes to the directory of its session
//! before its first prompt, without input in its terminal.
//!
//! The shell runs the start files of the user in the home directory, while
//! the holder waits in the pool. Then a start file of nomux reads the
//! directory of the session from the named pipe of the holder: the read
//! waits until a session takes the holder. The shell changes to the
//! directory and shows its first prompt there. The start files of nomux
//! have fixed content: the path of the named pipe is in a variable, and
//! the shell reads the directory as data. They leave no variable, no
//! function and no descriptor in the shell.
//!
//! - bash: `--rcfile` with a file that reads `~/.bashrc` first.
//! - zsh: `ZDOTDIR` with a `.zshenv` that gives `ZDOTDIR` of the user back,
//!   reads the `.zshenv` of the user, and adds a `precmd` function that
//!   runs one time.
//! - fish: `--init-command`, which runs after the configuration files.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use nix::sys::stat::Mode;
use nomux_holder_protocol::SessionId;

/// The variable with the path of the named pipe of the directory.
const DIRECTORY_FIFO: &str = "NOMUX_DIRECTORY_FIFO";
/// The variable with the `ZDOTDIR` of the user while `ZDOTDIR` names the
/// start files of nomux.
const USER_ZDOTDIR: &str = "NOMUX_ZDOTDIR";
const ZDOTDIR: &str = "ZDOTDIR";

/// The directory of the start files and the named pipes, in the runtime
/// directory.
const POOL_DIRECTORY: &str = "pool";
const BASH_START_FILE: &str = "bashrc";
const ZSH_DIRECTORY: &str = "zsh";
const OWNER_ONLY_DIRECTORY: u32 = 0o700;
const OWNER_READ_WRITE: u32 = 0o600;

const BASH_RC: &str = r#"# The start file of a bash in the holder pool of nomux. It reads the start
# file of the user, then changes to the directory of the session before the
# first prompt.
if [ -f ~/.bashrc ]; then . ~/.bashrc; fi
if [ -n "${NOMUX_DIRECTORY_FIFO-}" ]; then
  __nomux_oldpwd_set=${OLDPWD+set}
  __nomux_oldpwd=${OLDPWD-}
  if IFS= read -r -d '' __nomux_directory 2>/dev/null < "$NOMUX_DIRECTORY_FIFO"; then
    builtin cd -- "$__nomux_directory"
  fi
  if [ -n "$__nomux_oldpwd_set" ]; then OLDPWD=$__nomux_oldpwd; else unset OLDPWD; fi
  unset __nomux_directory __nomux_oldpwd __nomux_oldpwd_set
fi
unset NOMUX_DIRECTORY_FIFO
"#;

const ZSH_ENV: &str = r#"# The first start file of a zsh in the holder pool of nomux. It gives the
# start files of the user their ZDOTDIR back, reads the .zshenv of the user,
# and changes to the directory of the session before the first prompt.
if (( ${+NOMUX_ZDOTDIR} )); then
  ZDOTDIR=$NOMUX_ZDOTDIR
  unset NOMUX_ZDOTDIR
else
  unset ZDOTDIR
fi
if [[ -f ${ZDOTDIR:-$HOME}/.zshenv ]]; then
  builtin source -- "${ZDOTDIR:-$HOME}/.zshenv"
fi
__nomux_directory() {
  builtin emulate -L zsh
  precmd_functions=(${precmd_functions:#__nomux_directory})
  builtin unfunction __nomux_directory
  local fifo=$NOMUX_DIRECTORY_FIFO directory start=$PWD oldpwd=$OLDPWD
  unset NOMUX_DIRECTORY_FIFO
  [[ -n $fifo ]] || return 0
  if IFS= builtin read -r -d '' directory 2>/dev/null < "$fifo"; then
    builtin cd -q -- "$directory"
  fi
  # A zsh sets OLDPWD to the directory of its start.
  if [[ $oldpwd == $start ]]; then OLDPWD=$PWD; else OLDPWD=$oldpwd; fi
}
precmd_functions=(__nomux_directory $precmd_functions)
"#;

const FISH_INIT: &str = "if set -q NOMUX_DIRECTORY_FIFO
    if test -p $NOMUX_DIRECTORY_FIFO
        read -z -l __nomux_directory < $NOMUX_DIRECTORY_FIFO
        and builtin cd -- $__nomux_directory
    end
    set -e NOMUX_DIRECTORY_FIFO
end";

/// A shell that the pool can start with a start file of nomux.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartFileShell {
    Bash,
    Zsh,
    Fish,
}

impl StartFileShell {
    /// The kind of `shell` by the file name of the program. `None` if the
    /// pool cannot change the directory of the shell.
    pub(crate) fn of(shell: &OsStr) -> Option<Self> {
        match Path::new(shell).file_name()?.to_str()? {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

/// What the start of one pooled shell needs in addition to the shell.
pub(crate) struct ShellStart {
    /// The arguments after the shell. The session metadata does not have
    /// them: a session that starts again gets the shell without them.
    pub(crate) arguments: Vec<OsString>,
    /// The variables of the holder and its shell. `None` removes the
    /// variable.
    pub(crate) environment: Vec<(&'static str, Option<OsString>)>,
    /// The path of the named pipe that the holder opens.
    pub(crate) directory_fifo: PathBuf,
}

impl ShellStart {
    /// Writes the start files of nomux into the pool directory of
    /// `runtime_directory`, makes the named pipe of the holder of
    /// `session_id`, which only the user can open, and gives the start of a
    /// `shell` for that holder.
    pub(crate) fn prepare(
        shell: StartFileShell,
        runtime_directory: &Path,
        session_id: &SessionId,
    ) -> io::Result<Self> {
        let directory = runtime_directory.join(POOL_DIRECTORY);
        write_start_files(&directory)?;
        let directory_fifo = directory.join(format!("{session_id}.directory"));
        nix::unistd::mkfifo(&directory_fifo, Mode::S_IRUSR | Mode::S_IWUSR)?;
        let mut environment = vec![(DIRECTORY_FIFO, Some(directory_fifo.clone().into()))];
        let arguments = match shell {
            StartFileShell::Bash => {
                vec!["--rcfile".into(), directory.join(BASH_START_FILE).into()]
            }
            StartFileShell::Zsh => {
                environment.push((ZDOTDIR, Some(directory.join(ZSH_DIRECTORY).into())));
                environment.push((USER_ZDOTDIR, env::var_os(ZDOTDIR)));
                Vec::new()
            }
            StartFileShell::Fish => vec!["--init-command".into(), FISH_INIT.into()],
        };
        Ok(Self {
            arguments,
            environment,
            directory_fifo,
        })
    }
}

/// Removes the pool directory of `runtime_directory`: the start files and
/// the named pipes that an earlier daemon left.
pub(crate) fn remove_pool_directory(runtime_directory: &Path) -> io::Result<()> {
    match fs::remove_dir_all(runtime_directory.join(POOL_DIRECTORY)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Writes the start files of nomux. Only the user can read the directory.
fn write_start_files(directory: &Path) -> io::Result<()> {
    let zsh_directory = directory.join(ZSH_DIRECTORY);
    DirBuilder::new()
        .recursive(true)
        .mode(OWNER_ONLY_DIRECTORY)
        .create(&zsh_directory)?;
    write_file(&directory.join(BASH_START_FILE), BASH_RC)?;
    write_file(&zsh_directory.join(".zshenv"), ZSH_ENV)
}

fn write_file(path: &Path, content: &str) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(OWNER_READ_WRITE)
        .open(path)?
        .write_all(content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_file_name_of_the_shell_counts() {
        assert_eq!(
            StartFileShell::of(OsStr::new("/usr/local/bin/fish")),
            Some(StartFileShell::Fish)
        );
        assert_eq!(StartFileShell::of(OsStr::new("/bin/sh")), None);
        assert_eq!(StartFileShell::of(OsStr::new("/bin/bash-wrapper")), None);
    }
}
