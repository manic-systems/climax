// SPDX-License-Identifier: EUPL-1.2

use std::{
    collections::BTreeMap,
    fmt,
    str::FromStr,
};

/// A value a widget submits.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// No value.
    Null,
    /// A yes or no answer.
    Bool(bool),
    /// Text.
    String(String),
    /// An integer or floating point number.
    Number(Number),
    /// A calendar date.
    Date(Date),
    /// An ordered list of values.
    List(Vec<Self>),
    /// Named values, ordered by name.
    Object(BTreeMap<String, Self>),
}

impl Value {
    /// The boolean, if this is a `Bool`.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The text, if this is a `String`.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// The number, if this is a `Number`.
    #[must_use]
    pub const fn as_number(&self) -> Option<Number> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// The date, if this is a `Date`.
    #[must_use]
    pub const fn as_date(&self) -> Option<Date> {
        match self {
            Self::Date(value) => Some(*value),
            _ => None,
        }
    }

    /// The items, if this is a `List`.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(value) => Some(value),
            _ => None,
        }
    }

    /// The fields, if this is an `Object`.
    #[must_use]
    pub const fn as_object(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Number(Number::Integer(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Number(Number::Float(value))
    }
}

/// A number that keeps whether it was an integer.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Number {
    /// A whole number.
    Integer(i64),
    /// A floating point number.
    Float(f64),
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
        }
    }
}

/// A proleptic Gregorian calendar date.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Date {
    /// The year, negative before year zero.
    pub year:  i32,
    /// The month from 1 to 12.
    pub month: u8,
    /// The day of the month from 1.
    pub day:   u8,
}

impl Date {
    /// The date, or `None` when the month or day is out of range for that year.
    #[must_use]
    pub const fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if month < 1 || month > 12 {
            return None;
        }
        let max_day = days_in_month(year, month);
        if day < 1 || day > max_day {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// The date `days` after 1970-01-01, negative for earlier dates. A year
    /// beyond `i32` saturates.
    #[must_use]
    pub fn from_unix_days(days: i64) -> Self {
        let shifted = days.saturating_add(719_468);
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = (year_of_era + era * 400).saturating_add(i64::from(month <= 2));
        Self {
            year:  i32::try_from(year).unwrap_or(if year < 0 { i32::MIN } else { i32::MAX }),
            month: u8::try_from(month).unwrap_or(1),
            day:   u8::try_from(day).unwrap_or(1),
        }
    }

    /// Days since 1970-01-01, negative for earlier dates.
    #[must_use]
    pub fn unix_days(self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let month = i64::from(self.month);
        let day_of_year =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Date {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (negative, unsigned) = value
            .strip_prefix('-')
            .map_or((false, value), |rest| (true, rest));
        let mut parts = unsigned.split('-');
        let year = parts
            .next()
            .ok_or_else(|| invalid_date(value))?
            .parse::<i32>()
            .ok()
            .and_then(|year| {
                if negative {
                    year.checked_neg()
                } else {
                    Some(year)
                }
            })
            .ok_or_else(|| invalid_date(value))?;
        let month = parts
            .next()
            .ok_or_else(|| invalid_date(value))?
            .parse::<u8>()
            .map_err(|_error| invalid_date(value))?;
        let day = parts
            .next()
            .ok_or_else(|| invalid_date(value))?
            .parse::<u8>()
            .map_err(|_error| invalid_date(value))?;
        if parts.next().is_some() {
            return Err(invalid_date(value));
        }
        Self::new(year, month, day).ok_or_else(|| invalid_date(value))
    }
}

fn invalid_date(value: &str) -> String {
    format!("invalid date '{value}', expected YYYY-MM-DD")
}

pub(crate) const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_days_follow_the_epoch() {
        for (days, date) in [
            (0, Date::new(1970, 1, 1)),
            (19_723, Date::new(2024, 1, 1)),
            (19_782, Date::new(2024, 2, 29)),
            (-1, Date::new(1969, 12, 31)),
        ] {
            let date = date.unwrap();
            assert_eq!(Date::from_unix_days(days), date);
            assert_eq!(date.unix_days(), days);
        }
    }

    #[test]
    fn extreme_years_saturate_instead_of_overflowing() {
        let late = Date::new(6_000_000, 3, 1).unwrap();
        assert_eq!(Date::from_unix_days(late.unix_days()), late);
        assert_eq!(Date::from_unix_days(i64::MAX).year, i32::MAX);
        assert_eq!(Date::from_unix_days(i64::MIN).year, i32::MIN);
    }

    #[test]
    fn a_negative_year_round_trips_through_text() {
        let date = Date::new(-5, 3, 1).unwrap();
        assert_eq!(date.to_string().parse::<Date>(), Ok(date));
        assert!("--5-03-01".parse::<Date>().is_err());
    }
}
