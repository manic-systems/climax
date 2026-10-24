// SPDX-License-Identifier: EUPL-1.2

use screw::{
    RenderCtx,
    Role,
    Style,
    Surface,
};

use super::navigation::no_modifiers;
use crate::{
    Context,
    Date,
    Event,
    Key,
    Reaction,
    Value,
    Widget,
    WidgetId,
    value::days_in_month,
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

/// a calendar that picks one date with the arrow, page and home keys
#[derive(Clone, Debug)]
pub struct DatePicker {
    id:       WidgetId,
    selected: Date,
    today:    Option<Date>,
}

impl DatePicker {
    /// a picker with `selected` highlighted, out of range months and days are
    /// clamped
    #[must_use]
    pub fn new(id: impl Into<WidgetId>, selected: Date) -> Self {
        Self {
            id:       id.into(),
            selected: clamp_date(selected),
            today:    None,
        }
    }

    /// marks `today` in the calendar grid
    #[must_use]
    pub fn with_today(mut self, today: Date) -> Self {
        self.today = Some(clamp_date(today));
        self
    }

    /// the date currently highlighted
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
        let header: Vec<_> = WEEKDAYS.iter().map(|day| format!(" {day}")).collect();
        out.write(header.join(" "), theme.style(Role::Dim));

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

    fn has_key_help(&self) -> bool {
        true
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
            Key::Char(_)
            | Key::Tab
            | Key::Backtab
            | Key::Backspace
            | Key::Delete
            | Key::Insert
            | Key::Function(_) => Reaction::Ignored,
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
    let total = i64::from(date.year) * 12 + i64::from(date.month) - 1 + i64::from(months);
    let Ok(year) = i32::try_from(total.div_euclid(12)) else {
        return if months < 0 { Date::MIN } else { Date::MAX };
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        KeyEvent,
        Modifiers,
    };

    fn press(picker: &mut DatePicker, key: Key) -> Reaction {
        picker.handle(Event::key(key), &mut Context::new())
    }

    #[test]
    fn paging_stops_at_the_last_year() {
        let last = Date::new(i32::MAX, 12, 31).unwrap();
        let mut picker = DatePicker::new("when", last);
        assert_eq!(press(&mut picker, Key::PageDown), Reaction::Ignored);
        assert_eq!(press(&mut picker, Key::Right), Reaction::Ignored);
        assert_eq!(picker.selected(), last);

        let first = Date::new(i32::MIN, 1, 1).unwrap();
        let mut early = DatePicker::new("when", first);
        assert_eq!(press(&mut early, Key::PageUp), Reaction::Ignored);
        assert_eq!(press(&mut early, Key::Left), Reaction::Ignored);
        assert_eq!(early.selected(), first);
    }

    fn at(year: i32, month: u8, day: u8) -> DatePicker {
        DatePicker::new("when", Date::new(year, month, day).unwrap())
    }

    #[test]
    fn days_step_across_month_and_year_ends() {
        let mut picker = at(2023, 12, 31);
        press(&mut picker, Key::Right);
        assert_eq!(picker.selected(), Date::new(2024, 1, 1).unwrap());
        press(&mut picker, Key::Left);
        assert_eq!(picker.selected(), Date::new(2023, 12, 31).unwrap());
        press(&mut picker, Key::Down);
        assert_eq!(picker.selected(), Date::new(2024, 1, 7).unwrap());
        press(&mut picker, Key::Up);
        assert_eq!(picker.selected(), Date::new(2023, 12, 31).unwrap());
    }

    #[test]
    fn leap_years_have_a_february_29th() {
        let mut picker = at(2024, 2, 28);
        press(&mut picker, Key::Right);
        assert_eq!(picker.selected(), Date::new(2024, 2, 29).unwrap());
        press(&mut picker, Key::Right);
        assert_eq!(picker.selected(), Date::new(2024, 3, 1).unwrap());

        let mut common = at(2023, 2, 28);
        press(&mut common, Key::Right);
        assert_eq!(common.selected(), Date::new(2023, 3, 1).unwrap());

        let mut century = at(1900, 2, 28);
        press(&mut century, Key::Right);
        assert_eq!(century.selected(), Date::new(1900, 3, 1).unwrap());
    }

    #[test]
    fn month_steps_clamp_the_day() {
        let mut picker = at(2024, 1, 31);
        press(&mut picker, Key::PageDown);
        assert_eq!(picker.selected(), Date::new(2024, 2, 29).unwrap());
        press(&mut picker, Key::PageUp);
        assert_eq!(picker.selected(), Date::new(2024, 1, 29).unwrap());

        let mut leap_day = at(2024, 2, 29);
        leap_day.handle(
            Event::Key(KeyEvent::with_modifiers(Key::PageDown, Modifiers::SHIFT)),
            &mut Context::new(),
        );
        assert_eq!(leap_day.selected(), Date::new(2025, 2, 28).unwrap());
    }

    #[test]
    fn home_and_end_stop_at_the_month_edges() {
        let mut picker = at(2024, 2, 10);
        press(&mut picker, Key::End);
        assert_eq!(picker.selected(), Date::new(2024, 2, 29).unwrap());
        assert_eq!(press(&mut picker, Key::End), Reaction::Ignored);
        press(&mut picker, Key::Home);
        assert_eq!(picker.selected(), Date::new(2024, 2, 1).unwrap());
        assert_eq!(press(&mut picker, Key::Home), Reaction::Ignored);
    }

    #[test]
    fn the_epoch_and_the_day_before_it_are_reachable() {
        let mut picker = at(1970, 1, 1);
        press(&mut picker, Key::Left);
        assert_eq!(picker.selected(), Date::new(1969, 12, 31).unwrap());
        press(&mut picker, Key::Right);
        assert_eq!(picker.selected(), Date::new(1970, 1, 1).unwrap());
        assert_eq!(weekday_monday0(picker.selected()), 3);
    }
}
