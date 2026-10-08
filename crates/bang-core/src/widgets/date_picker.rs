// SPDX-License-Identifier: EUPL-1.2

use screw::{RenderCtx, Role, Style, Surface};

use super::navigation::no_modifiers;
use crate::{Context, Date, Event, Key, Reaction, Value, Widget, WidgetId, value::days_in_month};

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

/// A calendar that picks one date with the arrow, page and home keys.
#[derive(Clone, Debug)]
pub struct DatePicker {
    id:       WidgetId,
    selected: Date,
    today:    Option<Date>,
}

impl DatePicker {
    /// A picker with `selected` highlighted. Out of range months and days are clamped.
    #[must_use]
    pub fn new(id: impl Into<WidgetId>, selected: Date) -> Self {
        Self {
            id:       id.into(),
            selected: clamp_date(selected),
            today:    None,
        }
    }

    /// Mark `today` in the calendar grid.
    #[must_use]
    pub fn with_today(mut self, today: Date) -> Self {
        self.today = Some(clamp_date(today));
        self
    }

    /// The date currently highlighted.
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
}

impl screw::Widget for DatePicker {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let theme = ctx.theme();
        let first = Date {
            year:  self.selected.year,
            month: self.selected.month,
            day:   1,
        };
        let start_offset = i32::from(weekday_monday0(first));
        let grid_start = add_days(first, -start_offset);

        out.write(
            format!(
                "{} {}",
                MONTHS[usize::from(self.selected.month - 1)],
                self.selected.year
            ),
            theme.style(Role::Prompt),
        );
        out.newline();
        out.write(WEEKDAYS.join(" "), theme.style(Role::Dim));

        for week in 0..6 {
            out.newline();
            for weekday in 0..7 {
                if weekday > 0 {
                    out.write(" ", Style::default());
                }
                let date = add_days(grid_start, week * 7 + weekday);
                let (marker, role) = if date == self.selected {
                    (">", Role::Selected)
                } else if self.today == Some(date) {
                    ("*", Role::Success)
                } else if date.month == self.selected.month {
                    (" ", Role::Normal)
                } else {
                    (".", Role::Dim)
                };
                out.write(format!("{marker}{:>2}", date.day), theme.style(role));
            }
        }

        out.newline();
        out.write(
            "arrows move | pgup/pgdn month | home/end month edge | enter submit | esc cancel",
            theme.style(Role::Dim),
        );
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
    Date::from_unix_days(date.unix_days().saturating_add(i64::from(days)))
}

fn weekday_monday0(date: Date) -> u8 {
    u8::try_from((date.unix_days() + 3).rem_euclid(7)).unwrap_or(0)
}

