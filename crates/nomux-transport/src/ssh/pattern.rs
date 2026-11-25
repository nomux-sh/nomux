//! The patterns of OpenSSH: `*` is each sequence of characters, `?` is one
//! character, and a pattern that starts with `!` excludes what it matches.
//! `~/.ssh/config` and `known_hosts` use the same patterns.

/// Returns `true` if `text` matches a pattern of `patterns` and matches no
/// pattern that starts with `!`. Letters match without their case, as host
/// names do.
pub(crate) fn matches_list<'a>(patterns: impl IntoIterator<Item = &'a str>, text: &str) -> bool {
    let mut matched = false;
    for pattern in patterns {
        match pattern.strip_prefix('!') {
            Some(excluded) if matches(excluded, text) => return false,
            Some(_) => {}
            None => matched = matched || matches(pattern, text),
        }
    }
    matched
}

/// Returns `true` if all of `text` matches `pattern`.
pub(crate) fn matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let text: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut pattern_at, mut text_at) = (0, 0);
    // The position after the last `*`, and the text position that it took.
    let mut star: Option<(usize, usize)> = None;
    while text_at < text.len() {
        match pattern.get(pattern_at) {
            Some('*') => {
                star = Some((pattern_at + 1, text_at));
                pattern_at += 1;
            }
            Some(&expected) if expected == '?' || expected == text[text_at] => {
                pattern_at += 1;
                text_at += 1;
            }
            _ => {
                // The last `*` takes one more character.
                let Some((after_star, taken)) = star else {
                    return false;
                };
                star = Some((after_star, taken + 1));
                pattern_at = after_star;
                text_at = taken + 1;
            }
        }
    }
    pattern[pattern_at..].iter().all(|&rest| rest == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_match_as_in_openssh() {
        assert!(matches("*", ""));
        assert!(matches("*.example.com", "vps.example.com"));
        assert!(matches("*.example.com", "a.b.example.com"));
        assert!(!matches("*.example.com", "example.com"));
        assert!(matches("192.168.?.*", "192.168.1.20"));
        assert!(!matches("192.168.?.*", "192.168.10.20"));
        assert!(matches("a*b*c", "aXbXbXc"));
        assert!(!matches("a*b*c", "aXbXbX"));
        assert!(matches("VPS", "vps"));
        assert!(!matches("vps", "vps2"));
    }

    #[test]
    fn a_negated_pattern_excludes_and_does_not_match_alone() {
        assert!(matches_list(
            ["*.example.com", "!bad.example.com"],
            "ok.example.com"
        ));
        assert!(!matches_list(
            ["*.example.com", "!bad.example.com"],
            "bad.example.com"
        ));
        assert!(!matches_list(["!bad.example.com"], "ok.example.com"));
    }
}
