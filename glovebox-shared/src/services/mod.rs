pub mod activity;
pub mod appointment;
pub mod budget;
pub mod checklist;
pub mod costs;
pub mod incident;
pub mod mileage;
pub mod model_template;
pub mod need;
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
pub mod todo;
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
/// `created_at`/`updated_at` stamps are UTC (see [`now_stamp`]).
#[must_use]
pub fn today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// Writes `date` as `YYYY-MM-DD`, the form every page shows a date in.
#[must_use]
pub fn date_label(date: chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Drops repeated ids, keeping first-seen order.
pub(crate) fn dedup_ids(ids: &[i32]) -> Vec<i32> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

/// Checks that `value` holds something other than whitespace.
pub(crate) fn validate_not_blank(field: &str, value: &str) -> crate::error::DomainResult<()> {
    if value.trim().is_empty() {
        return Err(crate::error::DomainError::invalid(
            field,
            "must not be blank",
        ));
    }
    Ok(())
}

/// `value` without its surrounding whitespace, or `None` when nothing is
/// left, so blank text is stored and read as absent.
#[must_use]
pub fn trim_to_none(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod date_label_tests {
    #[test]
    fn date_label_pads_month_and_day() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        assert_eq!(super::date_label(date), "2026-03-07");
    }
}
