//! The rule for the owner and the mode of the nomux files on a host.

/// The paths that the script of the check lists, in its sequence, and what
/// the group and the other users can have there.
const PATHS: [(&str, Access); 4] = [
    ("~/.nomux", Access::NoWrite),
    ("~/.nomux/bin", Access::NoWrite),
    ("~/.nomux/run", Access::None),
    ("~/.nomux/run/nomuxd.sock", Access::None),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// The group and the other users cannot write: they cannot replace a
    /// binary.
    NoWrite,
    /// The group and the other users have no access: they cannot connect
    /// to the daemon.
    None,
}

/// Checks the output of `id -u` and of one `ls -ldn` for each path of
/// [`PATHS`], where a path that does not exist gives the line `missing`.
/// Returns what is wrong and how to correct it.
pub(super) fn check_permissions(listing: &str) -> Result<String, String> {
    let mut lines = listing.lines();
    let user = lines
        .next()
        .filter(|user| user.parse::<u32>().is_ok())
        .ok_or("the host did not give the user ID")?;
    let mut present = Vec::new();
    for (path, access) in PATHS {
        let line = lines
            .next()
            .ok_or_else(|| format!("the host did not list {path}"))?;
        if line == "missing" {
            continue;
        }
        // `drwx------ 2 1000 1000 ...`: the mode, the links, the owner.
        let mut fields = line.split_whitespace();
        let mode: Vec<char> = fields.next().unwrap_or_default().chars().take(10).collect();
        let owner = fields.nth(1).unwrap_or_default();
        if mode.len() != 10 {
            return Err(format!("the host did not give the mode of {path}"));
        }
        let mode_text: String = mode.iter().collect();
        if owner != user {
            return Err(format!(
                "{path} belongs to the user {owner}, not to the user {user} of the connection"
            ));
        }
        let (open, limit, correct_mode) = match access {
            Access::NoWrite => (mode[5] != '-' || mode[8] != '-', "can write to it", "go-w"),
            Access::None => (
                mode[4..].iter().any(|&bit| bit != '-'),
                "can use it",
                "go-rwx",
            ),
        };
        if open {
            return Err(format!(
                "{path} has the mode {mode_text}: other users {limit}. Run `chmod {correct_mode} \
                 {path}` on the host"
            ));
        }
        present.push(format!("{path} {mode_text}"));
    }
    if present.is_empty() {
        Ok("~/.nomux does not exist on the host".to_owned())
    } else {
        Ok(format!("private to the user: {}", present.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVATE: &str = "1000\n\
        drwx------ 4 1000 1000 4096 Nov 25 10:00 /home/ada/.nomux\n\
        drwx------. 3 1000 1000 4096 Nov 25 10:00 /home/ada/.nomux/bin\n\
        drwx------ 2 1000 1000 4096 Nov 25 10:00 /home/ada/.nomux/run\n\
        srw------- 1 1000 1000 0 Nov 25 10:00 /home/ada/.nomux/run/nomuxd.sock\n";

    #[test]
    fn directories_and_a_socket_that_only_the_user_can_use_pass() {
        let detail = check_permissions(PRIVATE).unwrap();
        assert!(detail.contains("~/.nomux/run drwx------"), "{detail}");

        // A host without a daemon and without binaries has nothing to check.
        assert!(check_permissions("1000\nmissing\nmissing\nmissing\nmissing\n").is_ok());
    }

    #[test]
    fn a_runtime_directory_that_other_users_can_open_fails_with_the_command_that_corrects_it() {
        let open = PRIVATE.replace(
            "drwx------ 2 1000 1000 4096 Nov 25 10:00 /home/ada/.nomux/run\n",
            "drwxr-xr-x 2 1000 1000 4096 Nov 25 10:00 /home/ada/.nomux/run\n",
        );

        let reason = check_permissions(&open).unwrap_err();

        assert!(
            reason.contains("~/.nomux/run has the mode drwxr-xr-x"),
            "{reason}"
        );
        assert!(reason.contains("chmod go-rwx ~/.nomux/run"), "{reason}");
    }

    #[test]
    fn a_binary_directory_can_be_readable_but_not_writable_for_other_users() {
        let readable = PRIVATE.replace("drwx------. 3", "drwxr-xr-x. 3");
        assert!(check_permissions(&readable).is_ok());

        let writable = PRIVATE.replace("drwx------. 3", "drwxrwxr-x. 3");
        let reason = check_permissions(&writable).unwrap_err();
        assert!(reason.contains("chmod go-w ~/.nomux/bin"), "{reason}");
    }

    #[test]
    fn a_path_of_a_different_user_fails() {
        let foreign = PRIVATE.replace("srw------- 1 1000", "srw------- 1 0");

        let reason = check_permissions(&foreign).unwrap_err();

        assert!(reason.contains("belongs to the user 0"), "{reason}");
    }
}
