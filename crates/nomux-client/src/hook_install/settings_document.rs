//! The JSON document of a settings file and the entries that hooks need in
//! it.

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

/// `true` if the array of `entry` has its value.
pub(super) fn contains(document: &Value, entry: &HookEntry) -> bool {
    matches!(document.pointer(&entry.pointer), Some(Value::Array(items)) if items.contains(&entry.value))
}

/// The tokens of a JSON pointer: `/a/b~1c` is `a` and `b/c`. `None` for a
/// pointer that does not start with `/`.
pub(super) fn tokens(pointer: &str) -> Option<Vec<String>> {
    let rest = pointer.strip_prefix('/')?;
    Some(
        rest.split('/')
            .map(|token| token.replace("~1", "/").replace("~0", "~"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_a_json_object() {
        assert!(parse(b"[1, 2]").is_err());
        assert!(parse(b"{ not json").is_err());
        assert_eq!(parse(b" \n").unwrap(), Value::Object(Map::new()));
    }

    #[test]
    fn a_pointer_token_has_its_escapes() {
        assert_eq!(
            tokens("/a~1b/c~0d"),
            Some(vec!["a/b".to_owned(), "c~d".to_owned()])
        );
        assert_eq!(tokens("no-slash"), None);
    }
}
