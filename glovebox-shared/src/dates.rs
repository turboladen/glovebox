//! The date vocabulary of the records: the event-time type and the parsers
//! for dates the owner types.

use std::{fmt, str::FromStr};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use sea_orm::DeriveValueType;
use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};

const DAY: &str = "%Y-%m-%d";
const TIME: &str = "%Y-%m-%d %H:%M:%S";
const TIME_WITH_T: &str = "%Y-%m-%dT%H:%M:%S";

/// When something happened, as far as the owner knows it: a calendar day, or
/// a date and time on the owner's own clock (no zone is recorded).
///
/// It is stored and serialized as `YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS`. It
/// also parses a `T` between the date and the time. A bare day orders before
/// any time on that day, which is the order of the stored text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, DeriveValueType)]
#[sea_orm(value_type = "String")]
pub enum DayOrTime {
    Day(NaiveDate),
    Time(NaiveDateTime),
}

impl DayOrTime {
    /// The calendar date, whichever form the value has.
    #[must_use]
    pub fn day(self) -> NaiveDate {
        match self {
            Self::Day(day) => day,
            Self::Time(at) => at.date(),
        }
    }

    fn time(self) -> Option<NaiveTime> {
        match self {
            Self::Day(_) => None,
            Self::Time(at) => Some(at.time()),
        }
    }
}

impl From<NaiveDate> for DayOrTime {
    fn from(day: NaiveDate) -> Self {
        Self::Day(day)
    }
}

impl Ord for DayOrTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.day(), self.time()).cmp(&(other.day(), other.time()))
    }
}

impl PartialOrd for DayOrTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for DayOrTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Day(day) => write!(f, "{}", day.format(DAY)),
            Self::Time(at) => write!(f, "{}", at.format(TIME)),
        }
    }
}

/// The text is neither a zero-padded date nor a zero-padded date and time.
#[derive(Debug, thiserror::Error)]
#[error("not a date or a date and time")]
pub struct NotADayOrTime;

// The parser alone also takes `2026-1-5`, a leading space and a signed year,
// so a value counts only when it prints back as the text it came from.
fn canonical_day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, DAY)
        .ok()
        .filter(|day| day.format(DAY).to_string() == value)
}

fn canonical_time(value: &str, format: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, format)
        .ok()
        .filter(|at| at.format(format).to_string() == value)
}

impl FromStr for DayOrTime {
    type Err = NotADayOrTime;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        canonical_day(value)
            .map(Self::Day)
            .or_else(|| canonical_time(value, TIME).map(Self::Time))
            .or_else(|| canonical_time(value, TIME_WITH_T).map(Self::Time))
            .ok_or(NotADayOrTime)
    }
}

impl Serialize for DayOrTime {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DayOrTime {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Reads a calendar date the owner typed for `field`.
///
/// # Errors
/// `Invalid` on `field` unless `value` is a real day written exactly
/// `YYYY-MM-DD`, with the month and day zero-padded.
pub fn parse_date(field: &str, value: &str) -> DomainResult<NaiveDate> {
    canonical_day(value)
        .ok_or_else(|| DomainError::invalid(field, "must be a date like 2027-05-01"))
}

/// Reads an event time the owner typed for `field`: a date (`YYYY-MM-DD`) or
/// a date and time (`YYYY-MM-DD HH:MM:SS`, or with a `T` between them).
///
/// # Errors
/// `Invalid` on `field` unless `value` is one of those forms with every part
/// zero-padded.
pub fn parse_day_or_time(field: &str, value: &str) -> DomainResult<DayOrTime> {
    value.parse().map_err(|NotADayOrTime| {
        DomainError::invalid(
            field,
            "must be a date like 2027-05-01 or a date and time like 2027-05-01 14:30:00",
        )
    })
}

/// The date and time on the server's clock, to the second, for an event time
/// the owner left out ("when the reading was taken").
#[must_use]
pub fn local_now() -> DayOrTime {
    let now = chrono::Local::now().naive_local();
    DayOrTime::Time(now.with_nanosecond(0).unwrap_or(now))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn a_day_and_a_time_print_as_they_are_stored() {
        for text in ["2026-01-05", "2026-01-05 08:03:00"] {
            let parsed: DayOrTime = text.parse().unwrap();
            assert_eq!(parsed.to_string(), text);
        }
        assert_eq!(
            "2026-01-05".parse::<DayOrTime>().unwrap(),
            DayOrTime::Day(ymd(2026, 1, 5))
        );
    }

    #[test]
    fn a_t_between_date_and_time_prints_as_a_space() {
        let parsed: DayOrTime = "2026-01-05T08:03:00".parse().unwrap();
        assert_eq!(parsed.to_string(), "2026-01-05 08:03:00");
    }

    #[test]
    fn day_is_the_calendar_date_of_either_form() {
        for text in ["2026-01-05", "2026-01-05 23:59:59"] {
            assert_eq!(text.parse::<DayOrTime>().unwrap().day(), ymd(2026, 1, 5));
        }
    }

    #[test]
    fn a_bare_day_sorts_before_any_time_on_that_day_as_the_stored_text_does() {
        let mut texts = [
            "2026-01-06",
            "2026-01-05 00:00:00",
            "2026-01-05",
            "2026-01-05 08:00:00",
            "2026-01-04 23:00:00",
        ];
        let mut values: Vec<DayOrTime> = texts.iter().map(|t| t.parse().unwrap()).collect();
        values.sort();
        texts.sort_unstable();
        let printed: Vec<String> = values.iter().map(ToString::to_string).collect();
        assert_eq!(printed, texts);
    }

    #[test]
    fn json_is_the_stored_text() {
        let value: DayOrTime = "2026-01-05 08:03:00".parse().unwrap();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, r#""2026-01-05 08:03:00""#);
        assert_eq!(serde_json::from_str::<DayOrTime>(&json).unwrap(), value);
    }

    #[test]
    fn parse_date_takes_only_a_zero_padded_real_day() {
        assert_eq!(parse_date("d", "2026-01-05").unwrap(), ymd(2026, 1, 5));
        for bad in [
            "2026-1-5",
            " 2026-01-05",
            "2026-01-05 ",
            "26-01-05",
            "+2026-01-05",
            "2026-02-30",
            "2026-01-05 08:03:00",
        ] {
            let err = parse_date("d", bad).unwrap_err().to_string();
            assert_eq!(err, "d: must be a date like 2027-05-01", "{bad:?}");
        }
    }

    #[test]
    fn parse_day_or_time_takes_only_zero_padded_forms() {
        for good in ["2026-01-05", "2026-01-05 08:03:00", "2026-01-05T08:03:00"] {
            parse_day_or_time("at", good).unwrap();
        }
        for bad in [
            "2026-1-5 8:3:0",
            "2026-01-05  08:03:00",
            "2026-01-05 8:03:00",
            "2026-01-05 25:00:00",
            "2026-02-30",
        ] {
            let err = parse_day_or_time("at", bad).unwrap_err().to_string();
            assert_eq!(
                err,
                "at: must be a date like 2027-05-01 or a date and time like 2027-05-01 14:30:00",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn local_now_is_a_time_on_todays_date() {
        let now = local_now();
        assert!(matches!(now, DayOrTime::Time(_)));
        assert_eq!(now.to_string().len(), 19);
    }
}
