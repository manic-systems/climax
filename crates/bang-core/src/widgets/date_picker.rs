// SPDX-License-Identifier: EUPL-1.2

use crate::{
    CalendarDay,
    CalendarView,
    CalendarWeek,
    Context,
    Date,
    Event,
    Key,
    KeyEvent,
    Reaction,
    Role,
    Span,
    Value,
    View,
    ViewContext,
    ViewId,
    Widget,
    WidgetId,
};

const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Clone, Debug)]
pub struct DatePicker {
    id:       WidgetId,
    selected: Date,
    today:    Option<Date>,
}

impl DatePicker {
    #[must_use]
    pub fn new(id: impl Into<WidgetId>, selected: Date) -> Self {
        Self {
            id:       id.into(),
            selected: clamp_date(selected),
            today:    None,
        }
    }

    #[must_use]
    pub fn with_today(mut self, today: Date) -> Self {
        self.today = Some(clamp_date(today));
        self
    }

    #[must_use]
    pub const fn selected(&self) -> Date {
        self.selected
    }

    fn move_days(&mut self, days: i32) -> Reaction {
        let next = add_days(self.selected, days);
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        Reaction::Changed
    }

    fn move_months(&mut self, months: i32) -> Reaction {
        let next = add_months(self.selected, months);
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        Reaction::Changed
    }

    fn move_years(&mut self, years: i32) -> Reaction {
        self.move_months(years.saturating_mul(12))
    }

    fn move_month_edge(&mut self, day: u8) -> Reaction {
        let next = Date {
            year:  self.selected.year,
            month: self.selected.month,
            day:   day.min(days_in_month(self.selected.year, self.selected.month)),
        };
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        Reaction::Changed
    }

    const fn submit(&self) -> Reaction {
        Reaction::Submit(Value::Date(self.selected))
    }

    fn calendar_view(&self) -> CalendarView {
        let first = Date {
            year:  self.selected.year,
            month: self.selected.month,
            day:   1,
        };
        let start_offset = i32::from(weekday_monday0(first));
        let grid_start = add_days(first, -start_offset);

        let weeks = (0..6)
            .map(|week| {
                CalendarWeek {
                    days: (0..7)
                        .map(|weekday| {
                            let date = add_days(grid_start, week * 7 + weekday);
                            CalendarDay {
                                date,
                                label: date.day.to_string(),
                                in_month: date.month == self.selected.month,
                                selected: date == self.selected,
                                today: self.today == Some(date),
                            }
                        })
                        .collect(),
                }
            })
            .collect();

        CalendarView {
            id: Some(ViewId::owned(format!("{}/calendar", self.id.as_str()))),
            year: self.selected.year,
            month: self.selected.month,
            month_label: format!(
                "{} {}",
                MONTHS[usize::from(self.selected.month - 1)],
                self.selected.year
            ),
            weekdays: WEEKDAYS.iter().map(ToString::to_string).collect(),
            weeks,
            selected: self.selected,
            help: vec![Span::new(
                "arrows move | pgup/pgdn month | home/end month edge | enter submit | esc cancel",
                Role::Dim,
            )],
        }
    }
}

impl Widget for DatePicker {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
        let Event::Key(key) = event else {
            return Reaction::Ignored;
        };

        match key.key {
            Key::Left => self.move_days(-1),
            Key::Right => self.move_days(1),
            Key::Up => self.move_days(-7),
            Key::Down => self.move_days(7),
            Key::Home => self.move_month_edge(1),
            Key::End => {
                self.move_month_edge(days_in_month(self.selected.year, self.selected.month))
            },
            Key::PageUp => {
                if key.modifiers.contains(crate::Modifiers::SHIFT) {
                    self.move_years(-1)
                } else {
                    self.move_months(-1)
                }
            },
            Key::PageDown => {
                if key.modifiers.contains(crate::Modifiers::SHIFT) {
                    self.move_years(1)
                } else {
                    self.move_months(1)
                }
            },
            Key::Char('h' | 'H') if no_modifiers(&key) => self.move_days(-1),
            Key::Char('l' | 'L') if no_modifiers(&key) => self.move_days(1),
            Key::Char('k' | 'K') if no_modifiers(&key) => self.move_days(-7),
            Key::Char('j' | 'J') if no_modifiers(&key) => self.move_days(7),
            Key::Enter => self.submit(),
            Key::Esc => Reaction::Cancel,
            _ => Reaction::Ignored,
        }
    }

    fn view(&self, _cx: &ViewContext) -> View {
        View::Calendar(self.calendar_view())
    }

    fn current_value(&self) -> Option<Value> {
        Some(Value::Date(self.selected))
    }
}

fn clamp_date(date: Date) -> Date {
    let month = date.month.clamp(1, 12);
    Date {
        year: date.year,
        month,
        day: date.day.clamp(1, days_in_month(date.year, month)),
    }
}

fn add_months(date: Date, months: i32) -> Date {
    let zero_month = i32::from(date.month) - 1;
    let total = date
        .year
        .saturating_mul(12)
        .saturating_add(zero_month)
        .saturating_add(months);
    let year = total.div_euclid(12);
    let month = u8::try_from(total.rem_euclid(12) + 1).unwrap_or(1);
    Date {
        year,
        month,
        day: date.day.min(days_in_month(year, month)),
    }
}

fn add_days(date: Date, days: i32) -> Date {
    civil_from_days(days_from_civil(date.year, date.month, date.day).saturating_add(days))
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i32 {
    let year = year - i32::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i32::from(month);
    let day = i32::from(day);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i32) -> Date {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    Date {
        year:  year + i32::from(month <= 2),
        month: u8::try_from(month).unwrap_or(1),
        day:   u8::try_from(day).unwrap_or(1),
    }
}

fn weekday_monday0(date: Date) -> u8 {
    u8::try_from((days_from_civil(date.year, date.month, date.day) + 3).rem_euclid(7)).unwrap_or(0)
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

const fn no_modifiers(key: &KeyEvent) -> bool {
    key.modifiers.bits() == 0
}
