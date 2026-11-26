//! The addresses of a release archive: the rules for an `https` address
//! that the user gives, and the release of an extension of the organization
//! for its short form (`git`, `git@0.1.0`).
//!
//! The release of an extension of the organization is in the repository
//! `nomux-sh/extensions`: the tag is `<name>@<version>`, and the release has
//! one archive for each target and the file `SHA256SUMS`.

use serde::Deserialize;

/// The releases of the extensions of the organization.
const RELEASES_URL: &str = "https://github.com/nomux-sh/extensions/releases/download";
/// The list of the releases, newest first, from the GitHub API.
pub(crate) const RELEASE_LIST_URL: &str =
    "https://api.github.com/repos/nomux-sh/extensions/releases?per_page=100";
pub(crate) const CHECKSUM_FILE: &str = "SHA256SUMS";

/// An extension of the organization and, if the user gave one, its
/// version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShortForm {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
}

impl ShortForm {
    /// The short form in `text`: a name of lower-case letters, digits and
    /// `-`, and optionally `@` and a version `X.Y.Z`. `None` for each other
    /// text.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (name, version) = match text.split_once('@') {
            Some((name, version)) => (name, Some(version)),
            None => (text, None),
        };
        let name_valid = !name.is_empty()
            && !name.starts_with('-')
            && name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !name_valid || version.is_some_and(|version| parse_version(version).is_none()) {
            return None;
        }
        Some(Self {
            name: name.to_owned(),
            version: version.map(str::to_owned),
        })
    }
}

/// The address of the archive of the extension `name` at `version` for
/// `target`, and the address of its checksum file. Only fixed parts and the
/// parts that [`ShortForm::parse`] accepted make the addresses.
pub(crate) fn release_urls(name: &str, version: &str, target: &str) -> (String, String) {
    let release = format!("{RELEASES_URL}/{name}@{version}");
    (
        format!("{release}/nomux-{name}-{version}-{target}.tar.gz"),
        format!("{release}/{CHECKSUM_FILE}"),
    )
}

/// The newest version of the extension `name` in the release list
/// `list_json` of the GitHub API. Only the tags `<name>@X.Y.Z` count.
///
/// # Errors
///
/// Returns the reason if the list is not valid or has no release of the
/// extension.
pub(crate) fn newest_version(name: &str, list_json: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        #[serde(default)]
        draft: bool,
    }
    let releases: Vec<Release> = serde_json::from_slice(list_json).map_err(|error| {
        format!("the release list of nomux-sh/extensions is not valid: {error}")
    })?;
    releases
        .iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let version = release.tag_name.strip_prefix(name)?.strip_prefix('@')?;
            Some((parse_version(version)?, version))
        })
        .max()
        .map(|(_, version)| version.to_owned())
        .ok_or_else(|| {
            format!(
                "nomux-sh/extensions has no release of the extension {name}. The README of \
                 https://github.com/nomux-sh/extensions lists the extensions"
            )
        })
}

/// The address of the checksum file of the archive at `archive_url`: the
/// file `SHA256SUMS` in the same directory.
pub(crate) fn checksum_url(archive_url: &str) -> String {
    let directory = archive_url
        .rsplit_once('/')
        .map_or(archive_url, |(directory, _)| directory);
    format!("{directory}/{CHECKSUM_FILE}")
}

/// The file name at the end of `url`.
pub(crate) fn file_name(url: &str) -> &str {
    url.rsplit_once('/').map_or(url, |(_, name)| name)
}

/// Checks an address that the user gave: `https`, a host, no user name or
/// password, and no space or control character.
///
/// # Errors
///
/// Returns the reason if the address breaks a rule.
pub(crate) fn check_url(url: &str) -> Result<(), String> {
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(format!(
            "the address {url} does not start with https://: nomux downloads only with https"
        ));
    };
    if url
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!(
            "the address {url:?} has a space or a control character"
        ));
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(format!(
            "the address {url} has a user name or a password: give an address without them"
        ));
    }
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    if host.is_empty() {
        return Err(format!("the address {url} has no host"));
    }
    if !url.ends_with(".tar.gz") {
        return Err(format!(
            "the address {url} is not the address of a release archive (.tar.gz)"
        ));
    }
    Ok(())
}

/// The three numbers of a version `X.Y.Z`. `None` for each other text.
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut numbers = version.split('.').map(|part| {
        (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    });
    let version = (numbers.next()??, numbers.next()??, numbers.next()??);
    numbers.next().is_none().then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_form_is_a_name_and_an_optional_version() {
        let short = |name: &str, version: Option<&str>| ShortForm {
            name: name.to_owned(),
            version: version.map(str::to_owned),
        };
        assert_eq!(ShortForm::parse("git"), Some(short("git", None)));
        assert_eq!(
            ShortForm::parse("agent-claude@0.1.0"),
            Some(short("agent-claude", Some("0.1.0")))
        );
        for text in [
            "Git",
            "git@1.0",
            "git@latest",
            "../git",
            "git/x",
            "",
            "-git",
            "git@1.0.0.0",
        ] {
            assert_eq!(ShortForm::parse(text), None, "{text}");
        }
    }

    #[test]
    fn the_release_addresses_come_from_fixed_parts() {
        assert_eq!(
            release_urls("git", "0.1.0", "x86_64-unknown-linux-musl"),
            (
                "https://github.com/nomux-sh/extensions/releases/download/git@0.1.0/\
                 nomux-git-0.1.0-x86_64-unknown-linux-musl.tar.gz"
                    .to_owned(),
                "https://github.com/nomux-sh/extensions/releases/download/git@0.1.0/SHA256SUMS"
                    .to_owned()
            )
        );
    }

    #[test]
    fn the_newest_version_counts_only_the_tags_of_the_extension() {
        let list = br#"[
            {"tag_name": "git@0.9.0", "draft": false},
            {"tag_name": "git@0.10.1", "draft": false},
            {"tag_name": "git@0.11.0", "draft": true},
            {"tag_name": "git-lfs@9.0.0", "draft": false},
            {"tag_name": "git@1.0.0-rc.1", "draft": false},
            {"tag_name": "agent-claude@2.0.0", "draft": false}
        ]"#;

        assert_eq!(newest_version("git", list).unwrap(), "0.10.1");
        assert!(
            newest_version("none", list)
                .unwrap_err()
                .contains("no release")
        );
        assert!(newest_version("git", b"{}").is_err());
    }

    #[test]
    fn an_address_must_be_https_with_a_host_and_without_a_password() {
        assert!(check_url("https://example.com/a/nomux-x-1.0.0-t.tar.gz").is_ok());
        for (url, reason) in [
            (
                "http://example.com/a.tar.gz",
                "does not start with https://",
            ),
            ("file:///tmp/a.tar.gz", "does not start with https://"),
            (
                "https://user:secret@example.com/a.tar.gz",
                "a user name or a password",
            ),
            ("https:///a.tar.gz", "has no host"),
            (
                "https://example.com/a b.tar.gz",
                "a space or a control character",
            ),
            (
                "https://example.com/a.zip",
                "not the address of a release archive",
            ),
        ] {
            let message = check_url(url).unwrap_err();
            assert!(message.contains(reason), "{url}: {message}");
        }
        assert_eq!(
            checksum_url("https://example.com/r/nomux-x.tar.gz"),
            "https://example.com/r/SHA256SUMS"
        );
    }
}
