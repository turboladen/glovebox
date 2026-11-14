//! Dollar amounts typed by a person and the cents the database stores.

use crate::error::{DomainError, DomainResult};

/// Parses an amount the owner typed, such as `935`, `$935.00` or `1,234.5`,
/// into whole cents. Surrounding spaces, one leading `$`, a leading or trailing
/// decimal point and thousands commas (groups of exactly three digits) in the
/// dollar part are accepted.
///
/// # Errors
///
/// `Invalid` on the `amount` field for blank text, a negative amount, text
/// that is not a number, more than two decimal places, and a value too large
/// for `i32` cents.
pub fn parse_dollars(text: &str) -> DomainResult<i32> {
    let bad = |message: &str| DomainError::invalid("amount", format!("{text:?} {message}"));
    let trimmed = text.trim();
    let unsigned = trimmed.strip_prefix('$').unwrap_or(trimmed);
    if unsigned.is_empty() {
        return Err(DomainError::invalid("amount", "must not be blank"));
    }
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mut groups = whole.split(',');
    let first = groups.next().unwrap_or_default();
    let grouped_ok = first.len() <= 3 && digits(first) && groups.all(|g| g.len() == 3 && digits(g));
    // A comma is a thousands separator only: one to three digits, then
    // groups of exactly three. A lone decimal point is not an amount.
    let has_digits = !whole.is_empty() || !fraction.is_empty();
    let well_formed = if whole.contains(',') {
        grouped_ok
    } else {
        digits(whole)
    };
    if !has_digits || !well_formed || !digits(fraction) {
        return Err(bad("is not a dollar amount such as 935 or $1,234.50"));
    }
    let whole = whole.replace(',', "");
    if fraction.len() > 2 {
        return Err(bad("has more than two decimal places"));
    }
    let too_big = || bad("is too large");
    let dollars: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| too_big())?
    };
    let tenths_and_hundredths: i64 = format!("{fraction:0<2}").parse().unwrap_or(0);
    let cents = dollars
        .checked_mul(100)
        .and_then(|c| c.checked_add(tenths_and_hundredths))
        .ok_or_else(too_big)?;
    i32::try_from(cents).map_err(|_| too_big())
}

/// Formats whole cents as dollars with thousands separators, such as
/// `$1,234.50`. A negative amount carries a leading minus sign.
pub fn format_cents(cents: i32) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{sign}${grouped}.{:02}", abs % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dollars_table() {
        for (text, cents) in [
            ("935", 93_500),
            ("$935.00", 93_500),
            ("1,234.5", 123_450),
            (" 12.3 ", 1_230),
            ("0", 0),
            ("$0.05", 5),
            ("12.", 1_200),
            ("21,474,836.47", i32::MAX),
            (".50", 50),
            ("$.5", 50),
            ("999,999.99", 99_999_999),
        ] {
            assert_eq!(parse_dollars(text).unwrap(), cents, "{text:?}");
        }
        for text in [
            "",
            "   ",
            "$",
            "-5",
            "$-5",
            "-0.50",
            "abc",
            "12abc",
            "1.234",
            "1.2.3",
            "1 000",
            ".",
            "$.",
            "1,23",
            "1,,234",
            ",5",
            "5,",
            "84,99",
            "1234,567",
            "1,234,56",
            "1.2,3",
            "$$5",
            "21,474,836.48",
            "99999999999999999999",
        ] {
            let err = parse_dollars(text).unwrap_err();
            assert!(
                matches!(&err, DomainError::Invalid { field, .. } if field == "amount"),
                "{text:?} gave {err:?}"
            );
        }
    }

    #[test]
    fn format_cents_table() {
        for (cents, text) in [
            (0, "$0.00"),
            (5, "$0.05"),
            (8_499, "$84.99"),
            (93_500, "$935.00"),
            (100_000, "$1,000.00"),
            (123_450, "$1,234.50"),
            (123_456_789, "$1,234,567.89"),
            (-8_499, "-$84.99"),
            (i32::MIN, "-$21,474,836.48"),
        ] {
            assert_eq!(format_cents(cents), text);
        }
    }
}
