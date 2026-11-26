//! The JSON document of a settings file and the entries that hooks need in
//! it. The order of the keys of the file stays.

use serde_json::{Map, Value};

use crate::{ClientError, HookInstallPlan};

/// One value that an array of the document must contain.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct HookEntry {
    /// A JSON pointer (RFC 6901) to the array.
    pub(super) pointer: String,
    pub(super) value: Value,
}

/// The entries of `plan`.
///
/// # Errors
///
/// Returns [`ClientError::Failed`] if a pointer or a value is not valid.
pub(super) fn entries_of(plan: &HookInstallPlan) -> Result<Vec<HookEntry>, ClientError> {
    let invalid = |problem: String| {
        ClientError::Failed(format!(
            "the install plan of the adapter {} is not valid: {problem}",
            plan.extension_id
        ))
    };
    plan.entries
        .iter()
        .map(|entry| {
            if tokens(&entry.pointer).is_none() {
                return Err(invalid(format!(
                    "`{}` is not a JSON pointer to a value below the root",
                    entry.pointer
                )));
            }
            let value = serde_json::from_str(&entry.value_json)
                .map_err(|error| invalid(format!("a value is not JSON: {error}")))?;
            Ok(HookEntry {
                pointer: entry.pointer.clone(),
                value,
            })
        })
        .collect()
}

/// The document of a settings file. A file without content is an empty
/// object.
///
/// # Errors
///
/// Returns the reason if the content is not a JSON object.
pub(super) fn parse(content: &[u8]) -> Result<Value, String> {
    if content.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Object(Map::new()));
    }
    let document: Value = serde_json::from_slice(content).map_err(|error| error.to_string())?;
    if !document.is_object() {
        return Err("the document is not a JSON object".to_owned());
    }
    Ok(document)
}

/// The text of `document`: two spaces for each level and a line end at the
/// end.
pub(super) fn render(document: &Value) -> Vec<u8> {
    let mut text = serde_json::to_vec_pretty(document).expect("a JSON value can be written");
    text.push(b'\n');
    text
}

/// Adds the value of `entry` to its array, and each object and the array on
/// its path that the document does not have. A value that the array has
/// already is not added again.
///
/// # Errors
///
/// Returns the reason if a value on the path is not an object or the last
/// value is not an array.
pub(super) fn add(document: &mut Value, entry: &HookEntry) -> Result<(), String> {
    let tokens = tokens(&entry.pointer).expect("the entries of a plan have valid pointers");
    let (last, parents) = tokens
        .split_last()
        .expect("a pointer has one token or more");
    let mut current = document;
    for token in parents {
        current = object(current, &entry.pointer)?
            .entry(token.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let array = object(current, &entry.pointer)?
        .entry(last.clone())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Value::Array(items) = array else {
        return Err(format!("the value at `{}` is not an array", entry.pointer));
    };
    if !items.contains(&entry.value) {
        items.push(entry.value.clone());
    }
    Ok(())
}

/// Removes the value of `entry` from its array. Nothing else changes.
pub(super) fn remove(document: &mut Value, entry: &HookEntry) {
    if let Some(Value::Array(items)) = document.pointer_mut(&entry.pointer) {
        items.retain(|item| item != &entry.value);
    }
}

/// `true` if the array of `entry` has its value.
pub(super) fn contains(document: &Value, entry: &HookEntry) -> bool {
    matches!(document.pointer(&entry.pointer), Some(Value::Array(items)) if items.contains(&entry.value))
}

fn object<'value>(
    value: &'value mut Value,
    pointer: &str,
) -> Result<&'value mut Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("a value on the path `{pointer}` is not an object"))
}

/// The tokens of a JSON pointer: `/a/b~1c` is `a` and `b/c`. `None` for a
/// pointer that does not start with `/`.
fn tokens(pointer: &str) -> Option<Vec<String>> {
    let rest = pointer.strip_prefix('/')?;
    Some(
        rest.split('/')
            .map(|token| token.replace("~1", "/").replace("~0", "~"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(pointer: &str, value: Value) -> HookEntry {
        HookEntry {
            pointer: pointer.to_owned(),
            value,
        }
    }

    #[test]
    fn an_entry_makes_its_path_and_is_added_one_time() {
        let mut document = parse(br#"{"theme": "dark", "hooks": {"Stop": [1]}}"#).unwrap();
        let start = entry("/hooks/Start", json!({"command": "nomux hook t start"}));

        add(&mut document, &start).unwrap();
        add(&mut document, &start).unwrap();
        add(&mut document, &entry("/hooks/Stop", json!(2))).unwrap();

        assert_eq!(
            document,
            json!({
                "theme": "dark",
                "hooks": {"Stop": [1, 2], "Start": [{"command": "nomux hook t start"}]}
            })
        );
        // The keys keep the order of the file.
        let text = String::from_utf8(render(&document)).unwrap();
        assert!(text.find("theme") < text.find("hooks"), "{text}");
        assert!(text.find("Stop") < text.find("Start"), "{text}");
    }

    #[test]
    fn a_removal_removes_only_the_value_of_the_entry() {
        let mut document = json!({"hooks": {"Start": [1, 2, 1]}, "other": true});

        remove(&mut document, &entry("/hooks/Start", json!(1)));
        remove(&mut document, &entry("/missing/path", json!(1)));

        assert_eq!(document, json!({"hooks": {"Start": [2]}, "other": true}));
    }

    #[test]
    fn a_path_through_a_value_that_is_not_an_object_is_refused() {
        let mut document = json!({"hooks": [], "list": {"a": 1}});

        assert!(add(&mut document, &entry("/hooks/Start", json!(1))).is_err());
        assert!(add(&mut document, &entry("/list/a", json!(1))).is_err());
        assert!(parse(b"[1, 2]").is_err());
        assert!(parse(b"{ not json").is_err());
        assert_eq!(parse(b" \n").unwrap(), json!({}));
    }

    #[test]
    fn a_pointer_token_has_its_escapes() {
        let mut document = json!({});

        add(&mut document, &entry("/a~1b/c~0d", json!(1))).unwrap();

        assert_eq!(document, json!({"a/b": {"c~d": [1]}}));
        assert!(contains(&document, &entry("/a~1b/c~0d", json!(1))));
        assert_eq!(tokens("no-slash"), None);
    }
}
