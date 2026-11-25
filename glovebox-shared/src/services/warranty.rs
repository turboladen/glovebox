//! How a warranty reads on a given day. A warranty has an expiry date, an
//! expiry odometer reading or both, and it ends at whichever comes first.

use chrono::NaiveDate;
use serde::Serialize;

/// A warranty's bounds as of one day and one odometer reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WarrantyBounds {
    /// True while no bound the warranty has is passed.
    pub in_force: bool,
    /// Whether the day is after the expiry date. `None` without an expiry date.
    pub date_passed: Option<bool>,
    /// Whether the odometer is above the expiry reading. `None` without one.
    pub mileage_passed: Option<bool>,
    /// Days from the day to the expiry date, zero on the expiry date and
    /// after it. `None` without an expiry date.
    pub days_left: Option<i64>,
    /// Miles from the odometer to the expiry reading, zero at the reading and
    /// above it. `None` without an expiry reading.
    pub miles_left: Option<i32>,
}

/// Reads a warranty's bounds as of `today` and `mileage`, or `None` when it
/// has neither an expiry date nor an expiry reading. The expiry date and the
/// expiry reading themselves are still inside the warranty.
#[must_use]
pub fn read(
    expires_on: Option<NaiveDate>,
    expires_miles: Option<i32>,
    mileage: i32,
    today: NaiveDate,
) -> Option<WarrantyBounds> {
    if expires_on.is_none() && expires_miles.is_none() {
        return None;
    }
    let date_passed = expires_on.map(|on| today > on);
    let mileage_passed = expires_miles.map(|miles| mileage > miles);
    Some(WarrantyBounds {
        in_force: date_passed != Some(true) && mileage_passed != Some(true),
        date_passed,
        mileage_passed,
        days_left: expires_on.map(|on| Ord::max((on - today).num_days(), 0)),
        miles_left: expires_miles.map(|miles| Ord::max(miles.saturating_sub(mileage), 0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::date;

    fn today() -> NaiveDate {
        date("2026-11-24")
    }

    #[test]
    fn a_warranty_with_no_bound_has_no_reading() {
        assert_eq!(read(None, None, 50_000, today()), None);
    }

    #[test]
    fn a_date_bound_holds_through_its_last_day() {
        let on_the_day = read(Some(today()), None, 50_000, today()).unwrap();
        assert!(on_the_day.in_force);
        assert_eq!(on_the_day.date_passed, Some(false));
        assert_eq!(on_the_day.days_left, Some(0));
        assert_eq!(on_the_day.mileage_passed, None);
        assert_eq!(on_the_day.miles_left, None);

        let ahead = read(Some(date("2026-12-24")), None, 50_000, today()).unwrap();
        assert!(ahead.in_force);
        assert_eq!(ahead.days_left, Some(30));

        let behind = read(Some(date("2026-11-23")), None, 50_000, today()).unwrap();
        assert!(!behind.in_force);
        assert_eq!(behind.date_passed, Some(true));
        assert_eq!(behind.days_left, Some(0));
    }

    #[test]
    fn a_mileage_bound_holds_up_to_its_reading() {
        let at = read(None, Some(50_000), 50_000, today()).unwrap();
        assert!(at.in_force);
        assert_eq!(at.mileage_passed, Some(false));
        assert_eq!(at.miles_left, Some(0));
        assert_eq!(at.date_passed, None);
        assert_eq!(at.days_left, None);

        let under = read(None, Some(60_000), 48_500, today()).unwrap();
        assert!(under.in_force);
        assert_eq!(under.miles_left, Some(11_500));

        let over = read(None, Some(50_000), 50_001, today()).unwrap();
        assert!(!over.in_force);
        assert_eq!(over.mileage_passed, Some(true));
        assert_eq!(over.miles_left, Some(0));
    }

    #[test]
    fn a_warranty_with_both_bounds_ends_at_the_first_one_passed() {
        let both_hold = read(Some(date("2027-01-01")), Some(60_000), 50_000, today()).unwrap();
        assert!(both_hold.in_force);
        assert_eq!(both_hold.days_left, Some(38));
        assert_eq!(both_hold.miles_left, Some(10_000));

        let date_gone = read(Some(date("2016-01-01")), Some(60_000), 50_000, today()).unwrap();
        assert!(!date_gone.in_force);
        assert_eq!(date_gone.date_passed, Some(true));
        assert_eq!(date_gone.mileage_passed, Some(false));
        assert_eq!(date_gone.miles_left, Some(10_000));

        let miles_gone = read(Some(date("2027-01-01")), Some(40_000), 50_000, today()).unwrap();
        assert!(!miles_gone.in_force);
        assert_eq!(miles_gone.date_passed, Some(false));
        assert_eq!(miles_gone.mileage_passed, Some(true));
        assert_eq!(miles_gone.days_left, Some(38));
    }
}
