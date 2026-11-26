//! The edit of the text of a settings file at the positions of its values.
//! An install adds its entries with the indent and the spacing of the file,
//! and an uninstall removes them. Each other byte of the file stays.
//!
//! The settings file of Claude Code is strict JSON: the tool refuses a file
//! with a comment or a comma after the last value. Thus the text has no
//! comments to keep.

use std::ops::Range;

use serde::Serialize;
use serde_json::ser::{Formatter, PrettyFormatter, Serializer};
use serde_json::{Map, Value};

use super::json_spans::{Kind, Node, scan};
use super::settings_document::{HookEntry, tokens};

/// The text of a settings file that does not exist or has only whitespace.
pub(super) const EMPTY_DOCUMENT: &str = "{\n}\n";

/// `text` with the value of `entry` in its array, and each object and the
/// array on its path that `text` does not have. A value that the array has
/// already is not added again. The pointer is the first object or array
/// that the add made, if it made one.
///
/// # Errors
///
/// Returns the reason if a value on the path is not an object or the last
/// value is not an array.
pub(super) fn add(text: &str, entry: &HookEntry) -> Result<(String, Option<String>), String> {
    let root = scan(text).ok_or("the document is not JSON")?;
    let tokens = tokens(&entry.pointer).expect("the entries of a plan have valid pointers");
    let mut node = &root;
    for (index, token) in tokens.iter().enumerate() {
        let Kind::Object(members) = &node.kind else {
            return Err(format!(
                "a value on the path `{}` is not an object",
                entry.pointer
            ));
        };
        let Some(member) = members.iter().find(|member| &member.key == token) else {
            let value = new_value(&tokens[index + 1..], &entry.value);
            let style = Style::of(text, &root);
            let text = insert(text, node, &style, |indent| {
                let key = serde_json::to_string(token).expect("a key can be written");
                let colon = if indent.is_some() { ": " } else { style.colon };
                format!("{key}{colon}{}", style.render(&value, indent))
            });
            return Ok((text, Some(pointer_of(&tokens[..=index]))));
        };
        node = &member.value;
    }
    let Kind::Array(items) = &node.kind else {
        return Err(format!("the value at `{}` is not an array", entry.pointer));
    };
    if items.iter().any(|item| is_value(text, item, &entry.value)) {
        return Ok((text.to_owned(), None));
    }
    let style = Style::of(text, &root);
    let text = insert(text, node, &style, |indent| {
        style.render(&entry.value, indent)
    });
    Ok((text, None))
}

/// `text` without the value of `entry` in its array, and without each array
/// and object that has no other value than the one on the path to it and
/// that is in `made` (the objects and arrays that the install made, with
/// their content). For `None`, each such array and object goes. The root
/// object stays.
pub(super) fn remove(text: &str, entry: &HookEntry, made: Option<&[String]>) -> String {
    let mut text = text.to_owned();
    while let Some(range) = removal(&text, entry, made) {
        text.replace_range(range, "");
    }
    text
}

/// The bytes that the next removal for `entry` removes.
fn removal(text: &str, entry: &HookEntry, made: Option<&[String]>) -> Option<Range<usize>> {
    let root = scan(text)?;
    let tokens = tokens(&entry.pointer)?;
    let mut path = Vec::new();
    let mut node = &root;
    for token in &tokens {
        let Kind::Object(members) = &node.kind else {
            return None;
        };
        let index = members.iter().position(|member| &member.key == token)?;
        path.push((node, index));
        node = &members[index].value;
    }
    let Kind::Array(items) = &node.kind else {
        return None;
    };
    let mut index = items
        .iter()
        .position(|item| is_value(text, item, &entry.value))?;
    let removable = |depth: usize| {
        let pointer = pointer_of(&tokens[..depth]);
        made.is_none_or(|made| made.iter().any(|made| is_within(&pointer, made)))
    };
    let mut container = node;
    while child_spans(container).len() == 1 && removable(path.len()) {
        let Some((parent, parent_index)) = path.pop() else {
            break;
        };
        (container, index) = (parent, parent_index);
    }
    let spans = child_spans(container);
    Some(if spans.len() == 1 {
        container.span.start + 1..container.span.end - 1
    } else if index + 1 < spans.len() {
        spans[index].start..spans[index + 1].start
    } else {
        spans[index - 1].end..spans[index].end
    })
}

/// The JSON pointer of the tokens `tokens`.
fn pointer_of(tokens: &[String]) -> String {
    let mut pointer = String::new();
    for token in tokens {
        pointer.push('/');
        pointer.push_str(&token.replace('~', "~0").replace('/', "~1"));
    }
    pointer
}

/// `true` if the value at `pointer` is the value at `outer` or in it.
fn is_within(pointer: &str, outer: &str) -> bool {
    pointer
        .strip_prefix(outer)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The value at the end of the pointer tokens `rest` below a new key: an
/// array with `value`, in an object for each token.
fn new_value(rest: &[String], value: &Value) -> Value {
    match rest.split_first() {
        None => Value::Array(vec![value.clone()]),
        Some((token, rest)) => {
            Value::Object(Map::from_iter([(token.clone(), new_value(rest, value))]))
        }
    }
}

/// `text` with a new last child of the object or array `container`. The
/// child gets the indent of the other children of a container on more than
/// one line, and no line end in a container on one line.
fn insert(
    text: &str,
    container: &Node,
    style: &Style,
    child: impl Fn(Option<&str>) -> String,
) -> String {
    let spans = child_spans(container);
    let inner = container.span.start + 1..container.span.end - 1;
    let on_lines = text[container.span.clone()].contains('\n');
    let mut text = text.to_owned();
    match spans.last() {
        Some(last) if on_lines => {
            let indent = if starts_line(&text, last.start) {
                line_indent(&text, last.start).to_owned()
            } else {
                format!(
                    "{}{}",
                    line_indent(&text, container.span.start),
                    style.indent
                )
            };
            let added = format!(",\n{indent}{}", child(Some(&indent)));
            text.insert_str(last.end, &added);
        }
        Some(last) => {
            let comma = match spans.as_slice() {
                [first, second, ..] => text[first.end..second.start].to_owned(),
                _ => style.comma.to_owned(),
            };
            text.insert_str(last.end, &format!("{comma}{}", child(None)));
        }
        None if style.on_lines => {
            let outer = line_indent(&text, container.span.start).to_owned();
            let indent = format!("{outer}{}", style.indent);
            let added = format!("\n{indent}{}\n{outer}", child(Some(&indent)));
            text.replace_range(inner, &added);
        }
        None => text.replace_range(inner, &child(None)),
    }
    text
}

fn child_spans(node: &Node) -> Vec<Range<usize>> {
    match &node.kind {
        Kind::Object(members) => members.iter().map(|member| member.span.clone()).collect(),
        Kind::Array(items) => items.iter().map(|item| item.span.clone()).collect(),
        Kind::Scalar => Vec::new(),
    }
}

fn is_value(text: &str, node: &Node, value: &Value) -> bool {
    serde_json::from_str::<Value>(&text[node.span.clone()]).is_ok_and(|item| &item == value)
}

/// The whitespace at the start of the line of the byte `position`.
fn line_indent(text: &str, position: usize) -> &str {
    let start = text[..position].rfind('\n').map_or(0, |end| end + 1);
    let line = &text[start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// `true` if only whitespace comes before the byte `position` on its line.
fn starts_line(text: &str, position: usize) -> bool {
    let start = text[..position].rfind('\n').map_or(0, |end| end + 1);
    text[start..position].trim_matches([' ', '\t']).is_empty()
}

/// The format of a settings file: what new text copies.
struct Style {
    /// The indent of one level: from the first indented line.
    indent: String,
    /// `true` if the root object has more than one line.
    on_lines: bool,
    /// The text between a key and its value on one line.
    colon: &'static str,
    /// The text between two values on one line.
    comma: &'static str,
}

impl Style {
    const DEFAULT_INDENT: &str = "  ";

    fn of(text: &str, root: &Node) -> Self {
        let indent = text
            .split('\n')
            .skip(1)
            .map(|line| &line[..line.len() - line.trim_start_matches([' ', '\t']).len()])
            .find(|indent| !indent.is_empty())
            .unwrap_or(Self::DEFAULT_INDENT);
        let spaced = first_key_gap(text, root).is_none_or(|gap| gap.ends_with(' '));
        Self {
            indent: indent.to_owned(),
            on_lines: text[root.span.clone()].contains('\n'),
            colon: if spaced { ": " } else { ":" },
            comma: if spaced { ", " } else { "," },
        }
    }

    /// `value` on lines with `indent` before each line after the first, or
    /// on one line for `None`.
    fn render(&self, value: &Value, indent: Option<&str>) -> String {
        let Some(indent) = indent else {
            let formatter = OneLine {
                colon: self.colon,
                comma: self.comma,
            };
            return written(value, formatter);
        };
        let formatter = PrettyFormatter::with_indent(self.indent.as_bytes());
        written(value, formatter).replace('\n', &format!("\n{indent}"))
    }
}

fn written(value: &Value, formatter: impl Formatter) -> String {
    let mut bytes = Vec::new();
    value
        .serialize(&mut Serializer::with_formatter(&mut bytes, formatter))
        .expect("a JSON value can be written");
    String::from_utf8(bytes).expect("JSON is UTF-8")
}

/// The text between the first key of the document and its value, with the
/// colon.
fn first_key_gap<'text>(text: &'text str, node: &Node) -> Option<&'text str> {
    match &node.kind {
        Kind::Object(members) => members.first().map(|member| {
            let between = &text[member.span.start..member.value.span.start];
            &between[between.rfind('"').map_or(0, |quote| quote + 1)..]
        }),
        Kind::Array(items) => items.iter().find_map(|item| first_key_gap(text, item)),
        Kind::Scalar => None,
    }
}

/// A JSON value on one line with the spacing of the file.
struct OneLine {
    colon: &'static str,
    comma: &'static str,
}

impl Formatter for OneLine {
    fn begin_array_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            return Ok(());
        }
        writer.write_all(self.comma.as_bytes())
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            return Ok(());
        }
        writer.write_all(self.comma.as_bytes())
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<()> {
        writer.write_all(self.colon.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The entry of the Claude Code adapter for `event`.
    fn entry(event: &str) -> HookEntry {
        HookEntry {
            pointer: format!("/hooks/{event}"),
            value: json!({"hooks": [{"type": "command", "command": "nomux", "args": ["hook", "claude", event]}]}),
        }
    }

    /// `text` after an install of two events, and what the install made.
    fn installed(text: &str) -> (String, Vec<String>) {
        let mut made = Vec::new();
        let mut text = text.to_owned();
        for entry in [entry("Stop"), entry("SessionStart")] {
            let (added, pointer) = add(&text, &entry).unwrap();
            text = added;
            made.extend(pointer);
        }
        (text, made)
    }

    fn uninstalled(text: &str, made: Option<&[String]>) -> String {
        [entry("Stop"), entry("SessionStart")]
            .iter()
            .fold(text.to_owned(), |text, entry| remove(&text, entry, made))
    }

    /// Settings files as people write them: each indent, key order and end.
    const FILES: [&str; 7] = [
        "{\n    \"permissions\": {\n        \"allow\": [\"Bash(git status)\"]\n    },\n    \"hooks\": {\n        \"Stop\": [\n            {\n                \"hooks\": [{\"type\": \"command\", \"command\": \"notify-send done\"}]\n            }\n        ]\n    },\n    \"model\": \"opus\"\n}\n",
        "{\n\t\"model\": \"opus\",\n\t\"env\": {\n\t\t\"EDITOR\": \"vim\"\n\t}\n}",
        "{\"model\":\"opus\",\"hooks\":{}}",
        "{ \"hooks\": { \"Stop\": [ { \"hooks\": [ ] } ] }, \"model\": \"opus\" }\n",
        "{\n  \"hooks\": {\n    \"SessionStart\": []\n  },\n  \"$schema\": \"https://json.schemastore.org/claude-code-settings.json\"\n}\n\n",
        "{}",
        "\r\n{\r\n  \"zeta\": 1,\r\n  \"alpha\": {\"nested\": [1, 2, 3]}\r\n}\r\n",
    ];

    #[test]
    fn an_install_adds_lines_with_the_indent_of_the_file() {
        let (text, made) = installed(FILES[0]);

        assert_eq!(made, ["/hooks/SessionStart"]);
        assert_eq!(
            text,
            "{\n    \"permissions\": {\n        \"allow\": [\"Bash(git status)\"]\n    },\n    \"hooks\": {\n        \"Stop\": [\n            {\n                \"hooks\": [{\"type\": \"command\", \"command\": \"notify-send done\"}]\n            },\n            {\n                \"hooks\": [\n                    {\n                        \"type\": \"command\",\n                        \"command\": \"nomux\",\n                        \"args\": [\n                            \"hook\",\n                            \"claude\",\n                            \"Stop\"\n                        ]\n                    }\n                ]\n            }\n        ],\n        \"SessionStart\": [\n            {\n                \"hooks\": [\n                    {\n                        \"type\": \"command\",\n                        \"command\": \"nomux\",\n                        \"args\": [\n                            \"hook\",\n                            \"claude\",\n                            \"SessionStart\"\n                        ]\n                    }\n                ]\n            }\n        ]\n    },\n    \"model\": \"opus\"\n}\n"
        );
    }

    #[test]
    fn an_install_keeps_tabs_and_a_file_without_a_line_end() {
        let (text, _) = add(FILES[1], &entry("Stop")).unwrap();

        assert_eq!(
            text,
            "{\n\t\"model\": \"opus\",\n\t\"env\": {\n\t\t\"EDITOR\": \"vim\"\n\t},\n\t\"hooks\": {\n\t\t\"Stop\": [\n\t\t\t{\n\t\t\t\t\"hooks\": [\n\t\t\t\t\t{\n\t\t\t\t\t\t\"type\": \"command\",\n\t\t\t\t\t\t\"command\": \"nomux\",\n\t\t\t\t\t\t\"args\": [\n\t\t\t\t\t\t\t\"hook\",\n\t\t\t\t\t\t\t\"claude\",\n\t\t\t\t\t\t\t\"Stop\"\n\t\t\t\t\t\t]\n\t\t\t\t\t}\n\t\t\t\t]\n\t\t\t}\n\t\t]\n\t}\n}"
        );
    }

    #[test]
    fn an_install_in_a_file_on_one_line_stays_on_one_line() {
        assert_eq!(
            add(FILES[2], &entry("Stop")).unwrap().0,
            r#"{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"nomux","args":["hook","claude","Stop"]}]}]}}"#
        );
        assert_eq!(
            add(FILES[3], &entry("Stop")).unwrap().0,
            "{ \"hooks\": { \"Stop\": [ { \"hooks\": [ ] }, {\"hooks\": [{\"type\": \"command\", \"command\": \"nomux\", \"args\": [\"hook\", \"claude\", \"Stop\"]}]} ] }, \"model\": \"opus\" }\n"
        );
    }

    #[test]
    fn an_install_of_a_new_file_is_the_default_format() {
        assert_eq!(
            add(EMPTY_DOCUMENT, &entry("Stop")).unwrap().0,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&json!({"hooks": {"Stop": [entry("Stop").value]}}))
                    .unwrap()
            )
        );
    }

    #[test]
    fn an_uninstall_gives_each_file_its_bytes_again() {
        for file in FILES {
            let (text, made) = installed(file);
            let document: Value = serde_json::from_str(&text).unwrap();
            for entry in [entry("Stop"), entry("SessionStart")] {
                let array = document.pointer(&entry.pointer).unwrap();
                assert!(array.as_array().unwrap().contains(&entry.value), "{text}");
            }
            assert_eq!(installed(&text).0, text, "a second install changes nothing");

            assert_eq!(uninstalled(&text, Some(&made)), file, "{text}");
        }
    }

    #[test]
    fn an_uninstall_of_a_changed_file_removes_only_what_became_empty() {
        let (text, made) = installed("{\n  \"model\": \"opus\"\n}\n");
        assert_eq!(made, ["/hooks", "/hooks/SessionStart"]);
        // The user changes the file after the install.
        let changed = text.replace("\"opus\"", "\"sonnet\",\n  \"theme\": \"dark\"");
        assert_eq!(
            uninstalled(&changed, Some(&made)),
            "{\n  \"model\": \"sonnet\",\n  \"theme\": \"dark\"\n}\n"
        );

        // A list with an entry of the user, and a list and an object that
        // the user had without a value, stay.
        let file = "{\n  \"hooks\": {\n    \"Stop\": [\n      \"user\"\n    ],\n    \"SessionStart\": []\n  }\n}";
        let (text, made) = installed(file);
        assert_eq!(made, Vec::<String>::new());
        let changed = text.replace("\"user\"", "\"user\",\n      \"later\"");
        assert_eq!(
            uninstalled(&changed, Some(&made)),
            "{\n  \"hooks\": {\n    \"Stop\": [\n      \"user\",\n      \"later\"\n    ],\n    \"SessionStart\": []\n  }\n}"
        );

        // A record of an earlier version of nomux does not know what the
        // install made: each list and object without a value goes.
        assert_eq!(
            uninstalled(&text, None),
            "{\n  \"hooks\": {\n    \"Stop\": [\n      \"user\"\n    ]\n  }\n}"
        );
    }

    #[test]
    fn a_path_through_a_value_that_is_not_an_object_is_refused() {
        assert!(add(r#"{"hooks": []}"#, &entry("Stop")).is_err());
        assert!(add(r#"{"hooks": {"Stop": {}}}"#, &entry("Stop")).is_err());
    }
}
