pub mod activity;
pub mod appointment;
pub mod budget;
pub mod checklist;
pub mod costs;
pub mod finding;
pub mod history;
pub mod incident;
pub mod mileage;
pub mod milestone;
pub mod model_template;
pub mod need;
pub mod nhtsa;
pub mod part;
pub mod recall;
pub mod reminders;
pub mod research;
pub mod schedule;
pub mod search;
pub mod service_detail;
pub mod service_record;
pub mod shop;
pub mod todo;
pub mod upcoming;
pub mod vehicle;
pub mod vehicle_detail;
pub mod vin_decode;
pub mod waiting;
pub mod warranty;
pub mod wear;
pub mod work_item;
pub mod work_sheet;

/// Opens a transaction that holds SQLite's write lock from its `BEGIN`, so
/// everything it reads is the committed state and no other program can write
/// until it ends. A caller that writes inside the transaction uses this rather
/// than `begin`.
pub(crate) async fn begin_write<C: sea_orm::TransactionTrait>(
    db: &C,
) -> Result<C::Transaction, sea_orm::DbErr> {
    // A claim (a no-op UPDATE as the first statement) on a table with an FTS5
    // trigger fails with "database is locked" at once instead of waiting out
    // the busy timeout, so the lock is taken by the BEGIN itself.
    db.begin_with_options(sea_orm::TransactionOptions {
        sqlite_transaction_mode: Some(sea_orm::SqliteTransactionMode::Immediate),
        ..Default::default()
    })
    .await
}

/// The current instant in UTC, to the second, for a bookkeeping stamp
/// (`created_at`, `updated_at`). Whole seconds give every stored stamp the
/// same width, so the stored text sorts in time order.
pub(crate) fn now() -> chrono::DateTime<chrono::Utc> {
    use chrono::SubsecRound;
    chrono::Utc::now().trunc_subsecs(0)
}

/// The calendar date where the server runs. Every day count and every default
/// date value uses this, so "today" matches the owner's wall calendar. Stored
/// `created_at`/`updated_at` stamps are UTC.
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

/// Checks that a list's `limit`, when given, asks for at least one row. An
/// empty reply would read the same as a vehicle with no records.
pub(crate) fn validate_limit(limit: Option<u32>) -> crate::error::DomainResult<()> {
    if limit == Some(0) {
        return Err(crate::error::DomainError::invalid(
            "limit",
            "must be at least 1",
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
