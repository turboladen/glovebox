pub mod activity;
pub mod budget;
pub mod costs;
pub mod incident;
pub mod mileage;
pub mod model_template;
pub mod nhtsa;
pub mod part;
pub mod platform;
pub mod recall;
pub mod reminders;
pub mod research;
pub mod schedule;
pub mod search;
pub mod service_record;
pub mod shop;
pub mod upcoming;
pub mod vehicle;
pub mod vin_decode;
pub mod work_item;

/// The project-wide stored-timestamp stamp (naive UTC, stored as text).
pub(crate) fn now_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The calendar date where the server runs. Every day count and every default
/// date value uses this, so "today" matches the owner's wall calendar. Stored
/// `created_at`/`updated_at` stamps stay UTC (see [`now_stamp`]).
#[must_use]
pub fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// The server's local date and time as `YYYY-MM-DD HH:MM:SS`, for an event
/// time the owner left out ("when the reading was taken"). It is not a
/// bookkeeping stamp; those use [`now_stamp`].
pub(crate) fn local_now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Drops repeated ids, keeping first-seen order.
pub(crate) fn dedup_ids(ids: &[i32]) -> Vec<i32> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

const DATE_FORMAT: &str = "%Y-%m-%d";

// Stored dates are compared and sorted as text, so a value must be written
// exactly as its format prints it. The parser alone also takes `2026-1-5`, a
// leading space and a signed year.
fn is_canonical_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value, DATE_FORMAT)
        .is_ok_and(|date| date.format(DATE_FORMAT).to_string() == value)
}

fn is_canonical_datetime(value: &str, format: &str) -> bool {
    chrono::NaiveDateTime::parse_from_str(value, format)
        .is_ok_and(|at| at.format(format).to_string() == value)
}

/// Checks that `value` is a calendar date written exactly `YYYY-MM-DD`, with
/// the month and day zero-padded.
pub(crate) fn validate_date(field: &str, value: &str) -> crate::error::DomainResult<()> {
    if is_canonical_date(value) {
        Ok(())
    } else {
        Err(crate::error::DomainError::invalid(
            field,
            "must be a date like 2027-05-01",
        ))
    }
}

/// [`validate_date`] on a value that may be absent.
pub(crate) fn validate_optional_date(
    field: &str,
    value: Option<&str>,
) -> crate::error::DomainResult<()> {
    value.map_or(Ok(()), |v| validate_date(field, v))
}

/// Checks that `value` is a date (`YYYY-MM-DD`) or a date and time
/// (`YYYY-MM-DD HH:MM:SS`, or with a `T` between them), the forms a
/// timestamp column holds. Every part must be zero-padded.
pub(crate) fn validate_datetime(field: &str, value: &str) -> crate::error::DomainResult<()> {
    let ok = is_canonical_date(value)
        || is_canonical_datetime(value, "%Y-%m-%d %H:%M:%S")
        || is_canonical_datetime(value, "%Y-%m-%dT%H:%M:%S");
    if ok {
        Ok(())
    } else {
        Err(crate::error::DomainError::invalid(
            field,
            "must be a date like 2027-05-01 or a date and time like 2027-05-01 14:30:00",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_must_be_written_zero_padded() {
        validate_date("d", "2026-01-05").unwrap();
        for bad in [
            "2026-1-5",
            " 2026-01-05",
            "2026-01-05 ",
            "26-01-05",
            "+2026-01-05",
            "2026-02-30",
        ] {
            assert!(validate_date("d", bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn a_datetime_must_be_written_zero_padded() {
        for good in ["2026-01-05", "2026-01-05 08:03:00", "2026-01-05T08:03:00"] {
            validate_datetime("at", good).unwrap();
        }
        for bad in [
            "2026-1-5 8:3:0",
            "2026-01-05  08:03:00",
            "2026-01-05 8:03:00",
            "2026-01-05 25:00:00",
        ] {
            assert!(
                validate_datetime("at", bad).is_err(),
                "{bad:?} must be refused"
            );
        }
    }
}
