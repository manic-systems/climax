// SPDX-License-Identifier: EUPL-1.2

use std::ops::Range;

use crate::{CursorAnchor, Date, ViewId};

/// Renderer-independent context supplied while a widget describes its view.
///
/// Physical dimensions belong to the renderer and are intentionally absent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct ViewContext {}

/// Renderer-neutral feedback about the logical items presented in a list.
///
/// All ranges and page targets index the corresponding [`ListView::rows`].
/// Renderers produce this after physical layout so widgets can navigate by
/// actual terminal rows without depending on terminal measurement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListPresentation {
    pub id: ViewId,
    pub visible: Range<usize>,
    pub fully_visible: Range<usize>,
    pub page_up: Option<usize>,
    pub page_down: Option<usize>,
}

/// Feedback returned by a renderer after laying out a semantic view.
///
/// A session makes the latest presentation available to widgets while they
/// handle the next event. Empty feedback is valid for non-layout renderers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Presentation {
    pub lists: Vec<ListPresentation>,
}

impl Presentation {
    #[must_use]
    pub fn list(&self, id: &ViewId) -> Option<&ListPresentation> {
        self.lists.iter().find(|list| list.id == *id)
    }
}

/// A renderer-neutral description of a widget's current presentation.
///
/// This is the sum type passed across the renderer adapter boundary. It
/// carries semantic roles and interaction metadata, but deliberately contains
/// no terminal escape sequences or renderer-native styles.
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Empty,
    Text(Vec<Span>),
    Line(Vec<Span>),
    Stack(Vec<Self>),
    List(ListView),
    TextInput(TextInputView),
    Calendar(CalendarView),
    Cursor(CursorPlacement),
}

/// A text fragment carrying a semantic role rather than a concrete style.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span {
    pub text: String,
    pub role: Role,
}

impl Span {
    #[must_use]
    pub fn new(text: impl Into<String>, role: Role) -> Self {
        Self {
            text: text.into(),
            role,
        }
    }

    #[must_use]
    pub fn normal(text: impl Into<String>) -> Self {
        Self::new(text, Role::Normal)
    }
}

/// Semantic intent which a renderer adapter maps to its own style system.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum Role {
    Prompt,
    #[default]
    Normal,
    Dim,
    Selected,
    Match,
    Error,
    Success,
}

/// All logical candidates for a list plus selection and paging intent.
///
/// The renderer decides which candidates fit its physical viewport and
/// returns that decision as [`ListPresentation`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListView {
    /// Stable identity used to associate renderer feedback with this list.
    pub id: Option<ViewId>,
    pub header: Vec<Span>,
    /// Logical candidates, before physical clipping or wrapping.
    pub rows: Vec<ListRow>,
    /// Selected candidate, indexed within `rows`.
    pub selected: Option<usize>,
    /// Logical start requested by the widget's retained scroll intent.
    pub requested_start: usize,
    /// Total logical candidates represented by this view.
    pub total: usize,
    /// Optional policy cap on candidates, independent of physical height.
    pub max_visible: Option<usize>,
    pub help: Vec<Span>,
}

/// One renderer-neutral row in a [`ListView`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListRow {
    pub id: Option<ViewId>,
    pub spans: Vec<Span>,
    pub selected: bool,
    pub checked: Option<bool>,
}

/// Text input presentation and logical cursor position.
///
/// `cursor` counts Unicode scalar values, not bytes or display columns. The
/// adapter is responsible for converting it to its output model's coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputView {
    pub id: Option<ViewId>,
    pub prompt: Vec<Span>,
    pub value: String,
    pub placeholder: Option<String>,
    pub cursor: usize,
    pub cursor_anchor: CursorAnchor,
    pub error: Option<String>,
}

/// Calendar presentation data prepared by a date widget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarView {
    pub id: Option<ViewId>,
    pub year: i32,
    pub month: u8,
    pub month_label: String,
    pub weekdays: Vec<String>,
    pub weeks: Vec<CalendarWeek>,
    pub selected: Date,
    pub help: Vec<Span>,
}

/// A display week in a [`CalendarView`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarWeek {
    pub days: Vec<CalendarDay>,
}

/// A display day and its semantic calendar state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarDay {
    pub date: Date,
    pub label: String,
    pub in_month: bool,
    pub selected: bool,
    pub today: bool,
}

/// A cursor placement relative to a stable view anchor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorPlacement {
    pub anchor: CursorAnchor,
    pub column: usize,
}

/// Produce an unstyled, deterministic snapshot of a view.
///
/// This is primarily useful for widget tests and non-terminal diagnostics. It
/// is not a replacement for a renderer adapter: styles, anchors, and cursor
/// placement are intentionally omitted.
#[must_use]
pub fn plain_snapshot(view: &View) -> String {
    let mut lines = Vec::new();
    render_plain(view, &mut lines);
    lines.join("\n")
}

fn render_plain(view: &View, lines: &mut Vec<String>) {
    match view {
        View::Empty | View::Cursor(_) => {},
        View::Text(spans) | View::Line(spans) => {
            lines.push(render_spans(spans));
        },
        View::Stack(children) => {
            for child in children {
                render_plain(child, lines);
            }
        },
        View::List(list) => {
            if !list.header.is_empty() {
                lines.push(render_spans(&list.header));
            }
            for row in &list.rows {
                let marker = if row.selected { ">" } else { " " };
                let checked = match row.checked {
                    Some(true) => "[x] ",
                    Some(false) => "[ ] ",
                    None => "",
                };
                lines.push(format!("{marker} {checked}{}", render_spans(&row.spans)));
            }
            if !list.help.is_empty() {
                lines.push(render_spans(&list.help));
            }
        },
        View::TextInput(input) => {
            let mut line = render_spans(&input.prompt);
            if input.value.is_empty() {
                if let Some(placeholder) = &input.placeholder {
                    line.push_str(placeholder);
                }
            } else {
                line.push_str(&input.value);
            }
            lines.push(line);
            if let Some(error) = &input.error {
                lines.push(error.clone());
            }
        },
        View::Calendar(calendar) => {
            lines.push(calendar.month_label.clone());
            lines.push(calendar.weekdays.join(" "));
            for week in &calendar.weeks {
                let days = week
                    .days
                    .iter()
                    .map(|day| {
                        let marker = if day.selected {
                            ">"
                        } else if day.today {
                            "*"
                        } else if day.in_month {
                            " "
                        } else {
                            "."
                        };
                        format!("{marker}{:>2}", day.label)
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                lines.push(days);
            }
            if !calendar.help.is_empty() {
                lines.push(render_spans(&calendar.help));
            }
        },
    }
}

fn render_spans(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        CalendarDay, CalendarView, CalendarWeek, ListRow, ListView, Role, Span, TextInputView,
        View, plain_snapshot,
    };
    use crate::{CursorAnchor, Date};

    fn date(day: u8) -> Date {
        Date::new(2026, 7, day).expect("test date is valid")
    }

    #[test]
    fn list_snapshot_exposes_selection_and_check_state() {
        let view = View::List(ListView {
            id: None,
            header: vec![Span::new("Pick one", Role::Prompt)],
            rows: vec![
                ListRow {
                    id: None,
                    spans: vec![Span::normal("Alpha")],
                    selected: true,
                    checked: Some(true),
                },
                ListRow {
                    id: None,
                    spans: vec![Span::normal("Beta")],
                    selected: false,
                    checked: Some(false),
                },
            ],
            selected: Some(0),
            requested_start: 0,
            total: 2,
            max_visible: None,
            help: vec![Span::new("enter to select", Role::Dim)],
        });

        assert_eq!(
            plain_snapshot(&view),
            "Pick one\n> [x] Alpha\n  [ ] Beta\nenter to select"
        );
    }

    #[test]
    fn text_input_snapshot_uses_value_and_reports_error() {
        let view = View::TextInput(TextInputView {
            id: None,
            prompt: vec![Span::new("Name: ", Role::Prompt)],
            value: "Ada".to_owned(),
            placeholder: Some("anonymous".to_owned()),
            cursor: 3,
            cursor_anchor: CursorAnchor::borrowed("name"),
            error: Some("already taken".to_owned()),
        });

        assert_eq!(plain_snapshot(&view), "Name: Ada\nalready taken");
    }

    #[test]
    fn calendar_snapshot_exposes_selected_today_and_outside_month_days() {
        let view = View::Calendar(CalendarView {
            id: None,
            year: 2026,
            month: 7,
            month_label: "July 2026".to_owned(),
            weekdays: vec!["Mo".to_owned(), "Tu".to_owned(), "We".to_owned()],
            weeks: vec![CalendarWeek {
                days: vec![
                    CalendarDay {
                        date: date(1),
                        label: "1".to_owned(),
                        in_month: true,
                        selected: true,
                        today: false,
                    },
                    CalendarDay {
                        date: date(2),
                        label: "2".to_owned(),
                        in_month: true,
                        selected: false,
                        today: true,
                    },
                    CalendarDay {
                        date: date(3),
                        label: "3".to_owned(),
                        in_month: false,
                        selected: false,
                        today: false,
                    },
                ],
            }],
            selected: date(1),
            help: vec![],
        });

        assert_eq!(plain_snapshot(&view), "July 2026\nMo Tu We\n> 1 * 2 . 3");
    }
}
