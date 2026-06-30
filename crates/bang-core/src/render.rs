// SPDX-License-Identifier: EUPL-1.2

use crate::{
    CursorAnchor,
    Date,
    Value,
    ViewId,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewContext {
    pub width:  Option<u16>,
    pub height: Option<u16>,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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

/// text fragment
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

/// text styles
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

#[derive(Clone, Debug, PartialEq)]
pub struct ListView {
    pub id:       Option<ViewId>,
    pub header:   Vec<Span>,
    pub rows:     Vec<ListRow>,
    pub selected: Option<usize>,
    pub offset:   usize,
    pub total:    usize,
    pub help:     Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListRow {
    pub id:       Option<ViewId>,
    pub spans:    Vec<Span>,
    pub value:    Value,
    pub selected: bool,
    pub checked:  Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputView {
    pub id:            Option<ViewId>,
    pub prompt:        Vec<Span>,
    pub value:         String,
    pub placeholder:   Option<String>,
    pub cursor:        usize,
    pub cursor_anchor: CursorAnchor,
    pub error:         Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarView {
    pub id:          Option<ViewId>,
    pub year:        i32,
    pub month:       u8,
    pub month_label: String,
    pub weekdays:    Vec<String>,
    pub weeks:       Vec<CalendarWeek>,
    pub selected:    Date,
    pub help:        Vec<Span>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarWeek {
    pub days: Vec<CalendarDay>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarDay {
    pub date:     Date,
    pub label:    String,
    pub in_month: bool,
    pub selected: bool,
    pub today:    bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorPlacement {
    pub anchor: CursorAnchor,
    pub column: usize,
}

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
