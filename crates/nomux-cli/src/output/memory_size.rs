//! The text of a number of bytes of memory.

const KIBIBYTE: u64 = 1024;
const MEBIBYTE: u64 = 1024 * KIBIBYTE;
const GIBIBYTE: u64 = 1024 * MEBIBYTE;

/// `bytes` in the largest binary unit that gives a value of 1 or more, with
/// one decimal place. The value is cut, not rounded: the text never shows
/// more memory than the processes use.
pub(super) fn memory_size(bytes: u64) -> String {
    let (unit, name) = match bytes {
        GIBIBYTE.. => (GIBIBYTE, "GiB"),
        MEBIBYTE.. => (MEBIBYTE, "MiB"),
        _ => (KIBIBYTE, "KiB"),
    };
    // 128 bits: 10 times the largest value of 64 bits does not fit in 64.
    let tenths = u128::from(bytes) * 10 / u128::from(unit);
    format!("{}.{} {name}", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unit_changes_at_1024_and_the_value_is_cut_to_one_decimal_place() {
        assert_eq!(memory_size(0), "0.0 KiB");
        assert_eq!(memory_size(1023), "0.9 KiB");
        assert_eq!(memory_size(MEBIBYTE - 1), "1023.9 KiB");
        assert_eq!(memory_size(MEBIBYTE), "1.0 MiB");
        assert_eq!(memory_size(64 * MEBIBYTE + 99 * KIBIBYTE), "64.0 MiB");
        assert_eq!(memory_size(GIBIBYTE + GIBIBYTE / 2), "1.5 GiB");
        assert_eq!(memory_size(u64::MAX), "17179869183.9 GiB");
    }
}
