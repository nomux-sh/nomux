//! An amount of memory in the configuration file: a number and a unit, for
//! example `512M` or `4G`. The units are powers of 1024.

/// Reads an amount of memory in bytes. Returns `None` if the text is not a
/// number of kibibytes (`K`), mebibytes (`M`) or gibibytes (`G`), or if the
/// amount is 0.
pub(super) fn parse(text: &str) -> Option<u64> {
    let unit = text.chars().next_back()?;
    let bytes_in_unit: u64 = match unit {
        'K' => 1 << 10,
        'M' => 1 << 20,
        'G' => 1 << 30,
        _ => return None,
    };
    let digits = &text[..text.len() - unit.len_utf8()];
    // `parse` accepts a sign, and an amount has none.
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number: u64 = digits.parse().ok()?;
    number.checked_mul(bytes_in_unit).filter(|bytes| *bytes > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_amount_of_memory_is_a_number_and_one_binary_unit() {
        assert_eq!(parse("512K"), Some(512 * 1024));
        assert_eq!(parse("512M"), Some(512 * 1024 * 1024));
        assert_eq!(parse("4G"), Some(4 * 1024 * 1024 * 1024));

        for text in ["", "G", "4", "4 G", "+4G", "4g", "4GB", "1.5G", "4T", "0G"] {
            assert_eq!(parse(text), None, "{text:?}");
        }
        assert_eq!(parse("18446744073709551615G"), None);
    }
}
