//! The expansion of an argument of the configuration: `~`, the `%` tokens
//! and `${VARIABLE}`.

use std::env;
use std::path::Path;

/// The values of the `%` tokens for one connection.
#[derive(Debug)]
pub(super) struct TokenValues<'a> {
    /// `%d`: the home directory of the local user.
    pub(super) home: &'a Path,
    /// `%u`: the local user.
    pub(super) local_user: &'a str,
    /// `%h`: the host name of the connection.
    pub(super) host_name: &'a str,
    /// `%n`: the host as the user wrote it.
    pub(super) original_host: &'a str,
    /// `%p`: the port.
    pub(super) port: u16,
    /// `%r`: the user on the host.
    pub(super) remote_user: &'a str,
    /// `%k`: the name of the host in the files of known hosts.
    pub(super) host_key_alias: &'a str,
}

/// Expands a path argument: a `~` at the start, then the tokens and the
/// variables.
pub(super) fn expand_path(text: &str, values: &TokenValues<'_>) -> Result<String, String> {
    let home = values.home.to_string_lossy();
    let with_home = match text.strip_prefix('~') {
        Some("") => home.into_owned(),
        Some(rest) if rest.starts_with('/') => format!("{home}{rest}"),
        Some(_) => return Err(format!("`{text}`: the home directory of a different user")),
        None => text.to_owned(),
    };
    expand(&with_home, values)
}

/// Expands the `%` tokens and `${VARIABLE}`.
pub(super) fn expand(text: &str, values: &TokenValues<'_>) -> Result<String, String> {
    let mut expanded = String::new();
    let mut rest = text;
    while let Some(position) = rest.find(['%', '$']) {
        expanded.push_str(&rest[..position]);
        let special = &rest[position..];
        rest = if let Some(after) = special.strip_prefix("${") {
            let (name, after) = after
                .split_once('}')
                .ok_or_else(|| format!("`{text}`: `${{` has no `}}`"))?;
            let value =
                env::var(name).map_err(|_| format!("`{text}`: the variable {name} is not set"))?;
            expanded.push_str(&value);
            after
        } else if let Some(after) = special.strip_prefix('%') {
            let token = after
                .chars()
                .next()
                .ok_or_else(|| format!("`{text}`: `%` has no token"))?;
            expanded.push_str(&token_value(token, values).ok_or_else(|| {
                format!("`{text}`: the SSH client of nomux does not support the token `%{token}`")
            })?);
            &after[token.len_utf8()..]
        } else {
            expanded.push('$');
            &special[1..]
        };
    }
    expanded.push_str(rest);
    Ok(expanded)
}

fn token_value(token: char, values: &TokenValues<'_>) -> Option<String> {
    Some(match token {
        '%' => "%".to_owned(),
        'd' => values.home.to_string_lossy().into_owned(),
        'u' => values.local_user.to_owned(),
        'h' => values.host_name.to_owned(),
        'n' => values.original_host.to_owned(),
        'p' => values.port.to_string(),
        'r' => values.remote_user.to_owned(),
        'k' => values.host_key_alias.to_owned(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> TokenValues<'static> {
        TokenValues {
            home: Path::new("/home/ada"),
            local_user: "ada",
            host_name: "vps.example.com",
            original_host: "vps",
            port: 2222,
            remote_user: "deploy",
            host_key_alias: "vps.example.com",
        }
    }

    #[test]
    fn the_tokens_and_the_home_directory_are_expanded() {
        assert_eq!(
            expand_path("~/.ssh/%r@%h-%p_%n_%u%%", &values()).unwrap(),
            "/home/ada/.ssh/deploy@vps.example.com-2222_vps_ada%"
        );
        assert_eq!(expand_path("~", &values()).unwrap(), "/home/ada");
        assert_eq!(
            expand_path("%d/keys/a$b", &values()).unwrap(),
            "/home/ada/keys/a$b"
        );
    }

    #[test]
    fn what_cannot_be_expanded_is_an_error_that_names_it() {
        assert!(expand("%C", &values()).unwrap_err().contains("%C"));
        assert!(expand_path("~other/key", &values()).is_err());
        assert!(
            expand("${NOMUX_TEST_VARIABLE_THAT_IS_NOT_SET}", &values())
                .unwrap_err()
                .contains("NOMUX_TEST_VARIABLE_THAT_IS_NOT_SET")
        );
    }
}
