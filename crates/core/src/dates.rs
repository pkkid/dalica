//! Calendar dates and local date-times without a time zone, as photos record them
//! (`data-model.md` §3: a folder's `"date": "2026-06-04"`, a photo's `"date": "2026-06-04T19:42:10"`).
//!
//! Parsed, validated and re-formatted, never copied as text (`security.md` §5.4). Accepted input:
//! ISO (`2026-06-04`, `2026-06-04T19:42:10`, or a space instead of `T`) and EXIF
//! (`2026:06:04 19:42:10`). Years run 1..=9999; EXIF's "unknown" `0000:00:00 00:00:00` is refused.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, excerpt};

/// A calendar day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

/// A local date and time to the second, with no time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DateTime {
    date: Date,
    hour: u8,
    minute: u8,
    second: u8,
}

fn is_leap(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Date, Error> {
        if (1..=9999).contains(&year) && (1..=days_in_month(year, month)).contains(&day) {
            Ok(Date { year, month, day })
        } else {
            Err(Error::InvalidDate(format!("{year:04}-{month:02}-{day:02}")))
        }
    }

    pub fn year(&self) -> u16 {
        self.year
    }

    pub fn month(&self) -> u8 {
        self.month
    }

    pub fn day(&self) -> u8 {
        self.day
    }

    /// Parses `YYYY-MM-DD` (or `YYYY:MM:DD` with `exif`).
    fn parse_with(s: &str, sep: u8) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b.get(4) != Some(&sep) || b.get(7) != Some(&sep) {
            return None;
        }
        let year = u16::try_from(number(s.get(0..4)?)?).ok()?;
        let month = u8::try_from(number(s.get(5..7)?)?).ok()?;
        let day = u8::try_from(number(s.get(8..10)?)?).ok()?;
        Date::new(year, month, day).ok()
    }
}

impl DateTime {
    pub fn new(date: Date, hour: u8, minute: u8, second: u8) -> Result<DateTime, Error> {
        if hour < 24 && minute < 60 && second < 60 {
            Ok(DateTime { date, hour, minute, second })
        } else {
            Err(Error::InvalidDate(format!("{date}T{hour:02}:{minute:02}:{second:02}")))
        }
    }

    pub fn date(&self) -> Date {
        self.date
    }

    pub fn hour(&self) -> u8 {
        self.hour
    }

    pub fn minute(&self) -> u8 {
        self.minute
    }

    pub fn second(&self) -> u8 {
        self.second
    }

    /// The UTC date-time of a Unix time in seconds, or `None` outside years 1..=9999. For file
    /// times when a photo records no date.
    pub fn from_unix_secs(secs: i64) -> Option<DateTime> {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days)?;
        let date = Date::new(u16::try_from(year).ok()?, month, day).ok()?;
        let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
        DateTime::new(date, u8::try_from(h).ok()?, u8::try_from(m).ok()?, u8::try_from(s).ok()?).ok()
    }
}

/// Year, month, day of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> Option<(i64, u8, u8)> {
    let z = days.checked_add(719_468)?;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe.checked_add(era.checked_mul(400)?)? + i64::from(month <= 2);
    Some((year, u8::try_from(month).ok()?, u8::try_from(day).ok()?))
}

/// A fixed-width run of ASCII digits.
fn number(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl FromStr for Date {
    type Err = Error;

    fn from_str(s: &str) -> Result<Date, Error> {
        Date::parse_with(s, b'-').or_else(|| Date::parse_with(s, b':')).ok_or_else(|| Error::InvalidDate(excerpt(s)))
    }
}

impl FromStr for DateTime {
    type Err = Error;

    fn from_str(s: &str) -> Result<DateTime, Error> {
        let bad = || Error::InvalidDate(excerpt(s));
        let (date, time) = (s.get(..10).ok_or_else(bad)?, s.get(10..).ok_or_else(bad)?);
        let tb = time.as_bytes();
        let date = match tb.first() {
            Some(b'T') => Date::parse_with(date, b'-'),
            Some(b' ') => Date::parse_with(date, b'-').or_else(|| Date::parse_with(date, b':')),
            _ => None,
        }
        .ok_or_else(bad)?;
        if tb.len() != 9 || tb.get(3) != Some(&b':') || tb.get(6) != Some(&b':') {
            return Err(bad());
        }
        let field = |r: std::ops::Range<usize>| time.get(r).and_then(number).and_then(|n| u8::try_from(n).ok()).ok_or_else(bad);
        DateTime::new(date, field(1..3)?, field(4..6)?, field(7..9)?).map_err(|_| bad())
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}T{:02}:{:02}:{:02}", self.date, self.hour, self.minute, self.second)
    }
}

macro_rules! string_serde {
    ($t:ty) => {
        impl TryFrom<String> for $t {
            type Error = Error;

            fn try_from(s: String) -> Result<$t, Error> {
                s.parse()
            }
        }

        impl From<$t> for String {
            fn from(v: $t) -> String {
                v.to_string()
            }
        }
    };
}

string_serde!(Date);
string_serde!(DateTime);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_and_exif_forms() {
        let dt: DateTime = "2026-06-04T19:42:10".parse().unwrap();
        assert_eq!(dt.to_string(), "2026-06-04T19:42:10");
        assert_eq!("2026:06:04 19:42:10".parse::<DateTime>().unwrap(), dt);
        assert_eq!("2026-06-04 19:42:10".parse::<DateTime>().unwrap(), dt);
        assert_eq!("2026-06-04".parse::<Date>().unwrap(), dt.date());
        assert_eq!("2026:06:04".parse::<Date>().unwrap().to_string(), "2026-06-04");
        assert_eq!(serde_json::to_string(&dt).unwrap(), "\"2026-06-04T19:42:10\"");
        assert_eq!(serde_json::from_str::<DateTime>("\"2026-06-04T19:42:10\"").unwrap(), dt);
    }

    #[test]
    fn refuses_impossible_and_malformed_dates() {
        for bad in [
            "0000:00:00 00:00:00",
            "2026-02-29T00:00:00",
            "2026-13-01T00:00:00",
            "2026-06-04T24:00:00",
            "2026-06-04T23:60:00",
            "2026-06-04T23:59:60",
            "2026:06:04T19:42:10",
            "2026-06-04T19:42",
            "2026-06-04T19:42:10Z",
            "+026-06-04T19:42:10",
            "2026-6-4T19:42:10",
            "２０２６-06-04T19:42:10",
            "",
        ] {
            assert!(bad.parse::<DateTime>().is_err(), "{bad:?}");
        }
        assert!("2024-02-29".parse::<Date>().is_ok());
        assert!("1900-02-29".parse::<Date>().is_err());
        assert!("2000-02-29".parse::<Date>().is_ok());
        assert!(serde_json::from_str::<Date>("\"2026-06-31\"").is_err());
    }

    #[test]
    fn orders_chronologically() {
        let a: DateTime = "2025-12-31T23:59:59".parse().unwrap();
        let b: DateTime = "2026-01-01T00:00:00".parse().unwrap();
        assert!(a < b);
    }

    #[test]
    fn unix_seconds_convert_to_utc() {
        assert_eq!(DateTime::from_unix_secs(0).unwrap().to_string(), "1970-01-01T00:00:00");
        assert_eq!(DateTime::from_unix_secs(1_780_602_130).unwrap().to_string(), "2026-06-04T19:42:10");
        assert_eq!(DateTime::from_unix_secs(-1).unwrap().to_string(), "1969-12-31T23:59:59");
        assert_eq!(DateTime::from_unix_secs(951_782_400).unwrap().to_string(), "2000-02-29T00:00:00");
        assert!(DateTime::from_unix_secs(i64::MAX).is_none());
        assert!(DateTime::from_unix_secs(i64::MIN).is_none());
        assert!(DateTime::from_unix_secs(-62_135_596_801).is_none()); // 0000-12-31T23:59:59
    }
}
