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

/// Checks that `value` is a calendar date written `YYYY-MM-DD`.
pub(crate) fn validate_date(field: &str, value: &str) -> crate::error::DomainResult<()> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| crate::error::DomainError::invalid(field, "must be a date like 2027-05-01"))
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
/// timestamp column holds.
pub(crate) fn validate_datetime(field: &str, value: &str) -> crate::error::DomainResult<()> {
    let ok = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
        || chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").is_ok()
        || chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").is_ok();
    if ok {
        Ok(())
    } else {
        Err(crate::error::DomainError::invalid(
            field,
            "must be a date like 2027-05-01 or a date and time like 2027-05-01 14:30:00",
        ))
    }
}
