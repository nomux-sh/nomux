//! The rules of a hook event that the CLI and the daemon share: the names
//! of a tool and of an event, and the size of the data of an event.

/// The largest number of bytes of the data of one hook event: the standard
/// input of the hook. A hook with a larger input sends the event without
/// its data.
pub const MAX_HOOK_PAYLOAD: usize = 256 * 1024;

/// The largest number of bytes of a tool name.
const MAX_TOOL_NAME_LENGTH: usize = 64;

/// The largest number of bytes of an event name.
const MAX_EVENT_NAME_LENGTH: usize = 128;

/// `true` if `name` is the name of a tool: a lower-case letter, then
/// lower-case letters, digits and `-`, 64 bytes at most. The manifest of an
/// adapter extension uses the same rule.
#[must_use]
pub fn is_tool_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_TOOL_NAME_LENGTH
        && bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// `true` if `name` is the name of an event, as the hook of a tool gives
/// it: 1 to 128 bytes of ASCII letters, digits, `-`, `_`, `.` and `:`.
#[must_use]
pub fn is_event_name(name: &str) -> bool {
    (1..=MAX_EVENT_NAME_LENGTH).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_name_is_a_lower_case_name() {
        assert!(is_tool_name("testtool"));
        assert!(is_tool_name("tool-2"));
        assert!(!is_tool_name(""));
        assert!(!is_tool_name("2tool"));
        assert!(!is_tool_name("Tool"));
        assert!(!is_tool_name("a/b"));
        assert!(!is_tool_name(&"a".repeat(65)));
    }

    #[test]
    fn an_event_name_has_no_space_and_no_path() {
        assert!(is_event_name("PreToolUse"));
        assert!(is_event_name("tool.start:1"));
        assert!(!is_event_name(""));
        assert!(!is_event_name("two words"));
        assert!(!is_event_name("../x"));
        assert!(!is_event_name(&"e".repeat(129)));
    }
}
