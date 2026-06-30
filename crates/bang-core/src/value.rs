// SPDX-License-Identifier: EUPL-1.2

use std::{
    collections::BTreeMap,
    fmt,
    str::FromStr,
};

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Null,
    Bool(bool),
    String(String),
    Number(Number),
    Date(Date),
    List(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl Value {
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_number(&self) -> Option<Number> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_date(&self) -> Option<Date> {
        match self {
            Self::Date(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&[Self]> {
        match self {
            Self::List(value) => Some(value),
            _ => None,
        }
    }

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

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Number {
    Integer(i64),
    Float(f64),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Date {
    pub year:  i32,
    pub month: u8,
    pub day:   u8,
}

impl Date {
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
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Date {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split('-');
        let year = parts
            .next()
            .ok_or_else(|| invalid_date(value))?
            .parse::<i32>()
            .map_err(|_error| invalid_date(value))?;
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

const fn days_in_month(year: i32, month: u8) -> u8 {
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
