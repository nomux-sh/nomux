//! One line of an OpenSSH configuration file: a keyword and its arguments.

/// A line that sets something.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Directive {
    /// The keyword in lower case. OpenSSH ignores the case of a keyword.
    pub(super) keyword: String,
    pub(super) arguments: Vec<String>,
}

/// Reads one line. Returns `None` for an empty line and for a comment.
///
/// The forms are `Keyword argument...` and `Keyword=argument...`. An
/// argument in double quotes can contain spaces. A `#` at the start of an
/// argument starts a comment.
pub(super) fn parse_line(line: &str) -> Result<Option<Directive>, &'static str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let keyword_end = line
        .find(|character: char| character.is_whitespace() || character == '=')
        .unwrap_or(line.len());
    let (keyword, rest) = line.split_at(keyword_end);
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest);
    Ok(Some(Directive {
        keyword: keyword.to_ascii_lowercase(),
        arguments: split_arguments(rest)?,
    }))
}

fn split_arguments(text: &str) -> Result<Vec<String>, &'static str> {
    let mut arguments = Vec::new();
    let mut characters = text.chars().peekable();
    loop {
        while characters
            .next_if(|character| character.is_whitespace())
            .is_some()
        {}
        match characters.peek() {
            None | Some('#') => return Ok(arguments),
            Some(_) => {}
        }
        let mut argument = String::new();
        let mut quoted = false;
        while let Some(&character) = characters.peek() {
            if character == '"' {
                quoted = !quoted;
            } else if character.is_whitespace() && !quoted {
                break;
            } else {
                argument.push(character);
            }
            characters.next();
        }
        if quoted {
            return Err("a double quote has no end");
        }
        arguments.push(argument);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `parse_line` gives for a line that sets something.
    fn directive(keyword: &str, arguments: &[&str]) -> Directive {
        Directive {
            keyword: keyword.to_owned(),
            arguments: arguments
                .iter()
                .map(|&argument| argument.to_owned())
                .collect(),
        }
    }

    #[test]
    fn the_forms_of_a_line_are_read() {
        let parsed = |line| parse_line(line).unwrap().unwrap();
        assert_eq!(parse_line("  # a comment"), Ok(None));
        assert_eq!(parse_line(""), Ok(None));
        assert_eq!(
            parsed("HostName vps.example.com"),
            directive("hostname", &["vps.example.com"])
        );
        assert_eq!(parsed("Port=2222"), directive("port", &["2222"]));
        assert_eq!(parsed("Port = 2222"), directive("port", &["2222"]));
        assert_eq!(
            parsed("Host a b  !c # mine"),
            directive("host", &["a", "b", "!c"])
        );
        assert_eq!(
            parsed("IdentityAgent \"~/Library/Group Containers/agent.sock\""),
            directive("identityagent", &["~/Library/Group Containers/agent.sock"])
        );
        assert!(parse_line("IdentityFile \"~/.ssh/key").is_err());
    }
}
