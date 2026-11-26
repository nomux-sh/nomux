//! A length of time in the configuration file: a number and a unit, for
//! example `90s`, `15m`, `24h` or `7d`.

use std::time::Duration;

/// Reads a length of time. Returns `None` if the text is not a number of
/// seconds (`s`), minutes (`m`), hours (`h`) or days (`d`).
pub(super) fn parse(text: &str) -> Option<Duration> {
    let unit = text.chars().next_back()?;
    let seconds_in_unit = match unit {
        's' => 1,
        'm' => 60,
        'h' => 60 * 60,
        'd' => 24 * 60 * 60,
        _ => return None,
    };
    let digits = &text[..text.len() - unit.len_utf8()];
    // `parse` accepts a sign, and a length of time has none.
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number: u64 = digits.parse().ok()?;
    number.checked_mul(seconds_in_unit).map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_of_time_is_a_number_and_one_unit() {
        assert_eq!(parse("0s"), Some(Duration::ZERO));
        assert_eq!(parse("90s"), Some(Duration::from_secs(90)));
        assert_eq!(parse("15m"), Some(Duration::from_mins(15)));
        assert_eq!(parse("24h"), Some(Duration::from_hours(24)));
        assert_eq!(parse("7d"), Some(Duration::from_hours(7 * 24)));

        for text in [
            "", "s", "24", "24 h", "+1h", "-1h", "1.5h", "1hh", "1w", "1é",
        ] {
            assert_eq!(parse(text), None, "{text:?}");
        }
        // A value that does not fit is not a small value.
        assert_eq!(parse("18446744073709551615d"), None);
    }
}
