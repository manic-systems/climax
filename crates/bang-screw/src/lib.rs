// SPDX-License-Identifier: EUPL-1.2

//! adapter from bang views to screw render surface

use std::io::{
    self,
    Write,
};

use bang_core::{
    CalendarView,
    ListView,
    Role as BangRole,
    Span,
    TextInputView,
    Value,
    View,
    Widget as BangWidget,
};
use bang_terminal::{
    RunOutcome,
    SessionRenderer,
    TerminalSize,
};
use screw::{
    Position,
    RenderCtx,
    RenderStats,
    Renderer,
    Role as ScrewRole,
    Style,
    Surface,
    Theme,
    Widget,
};
use unicode_width::UnicodeWidthChar as _;

#[derive(Clone, Debug, PartialEq)]
pub struct BangView {
    view: View,
}

impl BangView {
    #[must_use]
    pub const fn new(view: View) -> Self {
        Self { view }
    }

    #[must_use]
    pub const fn view(&self) -> &View {
        &self.view
    }

    #[must_use]
    pub fn into_view(self) -> View {
        self.view
    }
}

impl Widget for BangView {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        render_into_with_theme(&self.view, ctx.theme, out);
    }
}

pub struct RetainedRenderer<W> {
    renderer: Renderer<W>,
}

impl<W> RetainedRenderer<W>
where
    W: Write,
{
    pub const fn new(writer: W) -> Self {
        Self {
            renderer: Renderer::new(writer),
        }
    }

    #[must_use]
    pub fn width(mut self, width: usize) -> Self {
        self.renderer = self.renderer.width(width);
        self
    }

    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.renderer = self.renderer.theme(theme);
        self
    }

    pub const fn resize(&mut self, width: usize) {
        self.renderer.resize(width);
    }

    pub fn render(&mut self, view: &View) -> io::Result<RenderStats> {
        self.renderer.draw(&BangView::new(view.clone()))
    }

    pub fn clear(&mut self) -> io::Result<RenderStats> {
        self.renderer.clear()
    }

    pub fn into_inner(self) -> W {
        self.renderer.into_inner()
    }
}

/// `bang-terminal` session renderer backed by `screw`.
pub struct ScrewSessionRenderer<'a, W> {
    renderer: RetainedRenderer<&'a mut W>,
}

impl<'a, W> ScrewSessionRenderer<'a, W>
where
    W: Write,
{
    pub fn new(writer: &'a mut W) -> Self {
        let mut renderer = RetainedRenderer::new(writer);
        if let Some(size) = bang_terminal::terminal_size() {
            renderer = renderer.width(usize::from(size.cols));
        }
        Self { renderer }
    }

    pub fn clear(&mut self) -> io::Result<RenderStats> {
        self.renderer.clear()
    }
}

impl<W> SessionRenderer for ScrewSessionRenderer<'_, W>
where
    W: Write,
{
    fn render(&mut self, view: &View) -> io::Result<()> {
        self.renderer.render(view).map(|_stats| ())
    }

    fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
        self.renderer.resize(usize::from(size.cols));
        Ok(())
    }
}

/// Run a live inline terminal session using `screw` for rendering.
pub fn run_live_session(widget: impl BangWidget + 'static) -> Result<Value, String> {
    let terminal = bang_terminal::TerminalModeGuard::activate_stdin().map_err(|error| {
        if matches!(
            error.kind(),
            io::ErrorKind::Unsupported | io::ErrorKind::NotConnected
        ) {
            error.to_string()
        } else {
            format!("failed to enable terminal raw mode: {error}")
        }
    })?;
    let mut signals = bang_terminal::SignalGuard::install_terminal_handlers()
        .map_err(|error| format!("failed to install terminal signal handlers: {error}"))?;
    let stdin = io::stdin();
    let stderr = io::stderr();
    let mut stderr = stderr.lock();

    let mut screen = bang_terminal::InlineScreenGuard::enter(&mut stderr)
        .map_err(|error| format!("terminal I/O failed: {error}"))?;
    let mut renderer = ScrewSessionRenderer::new(screen.writer());
    let result = bang_terminal::drive_tty_session_with_signals(
        widget,
        stdin.lock(),
        &mut renderer,
        &mut signals,
    );
    let clear = renderer.clear();
    drop(renderer);
    let cleanup = screen.leave();
    let outcome = match (result, clear, cleanup) {
        (Ok(outcome), Ok(_stats), Ok(())) => outcome,
        (Err(error), ..) | (_, Err(error), _) | (_, _, Err(error)) => {
            return Err(format!("terminal I/O failed: {error}"));
        },
    };

    match outcome {
        RunOutcome::Submitted(value) => Ok(value),
        RunOutcome::Cancelled => Err("cancelled".to_owned()),
        RunOutcome::InputEnded => Err("input ended before submit".to_owned()),
        RunOutcome::Signalled(signal) => {
            drop(signals);
            drop(terminal);
            bang_terminal::restore_default_and_raise(signal)
                .map_err(|error| format!("failed to re-raise signal {signal}: {error}"))?;
            Err(format!("interrupted by signal {signal}"))
        },
    }
}

#[must_use]
pub fn render_surface(view: &View) -> Surface {
    render_surface_with_theme(view, Theme::default())
}

#[must_use]
pub fn render_surface_with_theme(view: &View, theme: Theme) -> Surface {
    let mut surface = Surface::new();
    render_into_with_theme(view, theme, &mut surface);
    surface
}

pub fn render_into(view: &View, out: &mut Surface) {
    render_into_with_theme(view, Theme::default(), out);
}

pub fn render_into_with_theme(view: &View, theme: Theme, out: &mut Surface) {
    let mut renderer = ViewRenderer::new(theme, out);
    renderer.render_view(view);
}

#[must_use]
pub const fn map_role(role: BangRole) -> ScrewRole {
    match role {
        BangRole::Prompt => ScrewRole::Prompt,
        BangRole::Dim => ScrewRole::Dim,
        BangRole::Selected => ScrewRole::Selected,
        BangRole::Match => ScrewRole::Match,
        BangRole::Error => ScrewRole::Error,
        BangRole::Success => ScrewRole::Success,
        _ => ScrewRole::Normal,
    }
}

struct ViewRenderer<'a> {
    theme:      Theme,
    out:        &'a mut Surface,
    wrote_line: bool,
}

impl<'a> ViewRenderer<'a> {
    const fn new(theme: Theme, out: &'a mut Surface) -> Self {
        Self {
            theme,
            out,
            wrote_line: false,
        }
    }

    fn render_view(&mut self, view: &View) {
        match view {
            View::Text(spans) | View::Line(spans) => {
                self.start_line();
                self.write_spans(spans);
            },
            View::Stack(children) => {
                for child in children {
                    self.render_view(child);
                }
            },
            View::List(list) => self.render_list(list),
            View::TextInput(input) => self.render_text_input(input),
            View::Calendar(calendar) => self.render_calendar(calendar),
            View::Cursor(cursor) => {
                self.out.set_cursor(Position {
                    row: self.out.height().saturating_sub(1),
                    col: cursor.column,
                });
            },
            _ => {},
        }
    }

    fn render_list(&mut self, list: &ListView) {
        if !list.header.is_empty() {
            self.start_line();
            self.write_spans(&list.header);
        }
        for row in &list.rows {
            self.start_line();
            let marker_role = if row.selected {
                BangRole::Selected
            } else {
                BangRole::Dim
            };
            self.write_text(if row.selected { "> " } else { "  " }, marker_role);
            if let Some(checked) = row.checked {
                self.write_text(if checked { "[x] " } else { "[ ] " }, marker_role);
            }
            self.write_spans(&row.spans);
        }
        if !list.help.is_empty() {
            self.start_line();
            self.write_spans(&list.help);
        }
    }

    fn render_text_input(&mut self, input: &TextInputView) {
        self.start_line();
        self.write_spans(&input.prompt);
        let prompt_width = self.out.current_col();
        let cursor_col = prompt_width + prefix_width(&input.value, input.cursor);

        if input.value.is_empty() {
            if let Some(placeholder) = &input.placeholder {
                self.write_text(placeholder, BangRole::Dim);
            }
        } else {
            self.write_text(&input.value, BangRole::Normal);
        }

        self.out.set_cursor(Position {
            row: self.out.height().saturating_sub(1),
            col: cursor_col,
        });

        if let Some(error) = &input.error {
            self.start_line();
            self.write_text(error, BangRole::Error);
        }
    }

    fn render_calendar(&mut self, calendar: &CalendarView) {
        self.start_line();
        self.write_text(&calendar.month_label, BangRole::Prompt);
        self.start_line();
        self.write_text(calendar.weekdays.join(" "), BangRole::Dim);

        for week in &calendar.weeks {
            self.start_line();
            for (index, day) in week.days.iter().enumerate() {
                if index > 0 {
                    self.write_style_text(" ", Style::default());
                }
                let (marker, role) = if day.selected {
                    (">", BangRole::Selected)
                } else if day.today {
                    ("*", BangRole::Success)
                } else if day.in_month {
                    (" ", BangRole::Normal)
                } else {
                    (".", BangRole::Dim)
                };
                self.write_text(format!("{marker}{:>2}", day.label), role);
            }
        }

        if !calendar.help.is_empty() {
            self.start_line();
            self.write_spans(&calendar.help);
        }
    }

    fn start_line(&mut self) {
        if self.wrote_line {
            self.out.newline();
        } else {
            self.wrote_line = true;
        }
    }

    fn write_spans(&mut self, spans: &[Span]) {
        for span in spans {
            self.write_text(&span.text, span.role);
        }
    }

    fn write_text(&mut self, text: impl AsRef<str>, role: BangRole) {
        self.write_style_text(text, self.theme.style(map_role(role)));
    }

    fn write_style_text(&mut self, text: impl AsRef<str>, style: Style) {
        self.out.write(text, style);
    }
}

fn prefix_width(value: &str, chars: usize) -> usize {
    value
        .chars()
        .take(chars)
        .map(|ch| ch.width().unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use std::io;

    use bang_core::{
        CalendarDay,
        CalendarView,
        CalendarWeek,
        CursorAnchor,
        CursorPlacement,
        Event,
        ListRow,
        ListView,
        Role,
        Session,
        Span,
        TextInputView,
        Value,
        View,
        ViewId,
        plain_snapshot,
        widgets::{
            Select,
            SelectItem,
        },
    };
    use screw::{
        Color,
        RenderStats,
        Style,
        Theme,
        render_plain,
    };

    use super::{
        BangView,
        RetainedRenderer,
        map_role,
        render_surface,
    };

    #[test]
    fn maps_semantic_roles_to_screw_roles() {
        assert_eq!(map_role(Role::Prompt), screw::Role::Prompt);
        assert_eq!(map_role(Role::Selected), screw::Role::Selected);
        assert_eq!(map_role(Role::Error), screw::Role::Error);
    }

    #[test]
    fn renders_list_view_to_screw_surface() {
        let view = View::List(ListView {
            id:       Some(ViewId::from("list")),
            header:   vec![Span::new("choose one", Role::Prompt)],
            rows:     vec![
                ListRow {
                    id:       Some(ViewId::from("row/0")),
                    spans:    vec![Span::normal("alpha")],
                    value:    "alpha".into(),
                    selected: true,
                    checked:  Some(true),
                },
                ListRow {
                    id:       Some(ViewId::from("row/1")),
                    spans:    vec![Span::normal("bravo")],
                    value:    "bravo".into(),
                    selected: false,
                    checked:  Some(false),
                },
            ],
            selected: Some(0),
            offset:   0,
            total:    2,
            help:     vec![Span::new("enter submit", Role::Dim)],
        });

        let widget = BangView::new(view.clone());

        assert_eq!(render_plain(&widget), plain_snapshot(&view));
        assert!(plain_snapshot(&view).starts_with("choose one\n"));
    }

    #[test]
    fn screw_renderer_localizes_list_selection_changes() {
        let first = BangView::new(list_view_with_selection(0));
        let second = BangView::new(list_view_with_selection(1));
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);

        let initial = renderer.render(first.view()).unwrap();
        let changed = renderer.render(second.view()).unwrap();

        assert_eq!(initial.changed_rows, 3);
        assert_eq!(changed.changed_rows, 2);
    }

    #[test]
    fn screw_renderer_redraws_visible_rows_when_widget_window_scrolls() {
        let widget = Select::new("choices", items()).with_page_size(3);
        let mut session = Session::new(widget);
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);

        let initial = renderer.render(&session.view()).unwrap();
        session.clear_dirty();
        session.handle(Event::key(bang_core::Key::Down));
        session.clear_dirty();
        session.handle(Event::key(bang_core::Key::Down));
        session.clear_dirty();
        session.handle(Event::key(bang_core::Key::Down));
        let scrolled = renderer.render(&session.view()).unwrap();
        let output = String::from_utf8(renderer.into_inner()).unwrap();

        assert_eq!(initial.changed_rows, 4);
        assert_eq!(scrolled.changed_rows, 3);
        assert!(output.contains("alpha"));
        assert!(output.contains("delta"));
    }

    #[test]
    fn retained_renderer_can_drive_a_complete_blocking_session_transcript() {
        let widget = Select::new("choices", items()).with_page_size(2);
        let mut renderer = RecordingRetainedRenderer::new(80);

        let outcome = bang_terminal::drive_blocking_session(
            widget,
            b"\x1b[B\x1b[B\r".as_slice(),
            &mut renderer,
        )
        .unwrap();
        let changed_rows = renderer
            .stats
            .iter()
            .map(|stats| stats.changed_rows)
            .collect::<Vec<_>>();
        let output = String::from_utf8(renderer.into_output()).unwrap();

        assert_eq!(
            outcome,
            bang_terminal::RunOutcome::Submitted(Value::from("charlie"))
        );
        assert_eq!(changed_rows, vec![3, 2, 2, 0]);
        assert!(output.contains("alpha"));
        assert!(output.contains("bravo"));
        assert!(output.contains("charlie"));
    }

    #[test]
    fn screw_renderer_localizes_text_input_edits_to_input_row() {
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);

        let initial = renderer.render(&text_input_view("ab", 2, None)).unwrap();
        let changed = renderer.render(&text_input_view("abc", 3, None)).unwrap();

        assert_eq!(initial.changed_rows, 1);
        assert_eq!(changed.changed_rows, 1);
    }

    #[test]
    fn retained_renderer_clear_removes_previous_prompt_block() {
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);

        renderer.render(&text_input_view("draft", 5, None)).unwrap();
        let cleared = renderer.clear().unwrap();
        let output = String::from_utf8(renderer.into_inner()).unwrap();

        assert!(cleared.changed_rows > 0);
        assert!(output.contains("\x1b[K"));
        assert!(!output.contains("\x1b[?1049"));
    }

    #[test]
    fn screw_renderer_localizes_validation_error_to_status_row() {
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);

        let initial = renderer.render(&text_input_view("abc", 3, None)).unwrap();
        let changed = renderer
            .render(&text_input_view("abc", 3, Some("too short")))
            .unwrap();

        assert_eq!(initial.changed_rows, 1);
        assert_eq!(changed.changed_rows, 2);
    }

    #[test]
    fn screw_renderer_tracks_cursor_only_changes_without_rewriting_rows() {
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);

        let initial = renderer.render(&text_input_view("abc", 3, None)).unwrap();
        let changed = renderer.render(&text_input_view("abc", 1, None)).unwrap();
        let output = String::from_utf8(renderer.into_inner()).unwrap();

        assert_eq!(initial.changed_rows, 1);
        assert_eq!(changed.changed_rows, 0);
        assert!(output.contains("\x1b[3C"));
    }

    #[test]
    fn screw_renderer_resize_forces_refit_redraw() {
        let mut renderer = RetainedRenderer::new(Vec::new()).width(80);
        let view = View::Line(vec![Span::normal("abcdef")]);

        let initial = renderer.render(&view).unwrap();
        renderer.resize(4);
        let resized = renderer.render(&view).unwrap();

        assert_eq!(initial.changed_rows, 1);
        assert_eq!(resized.changed_rows, 2);
        assert_eq!(render_surface(&view).plain_text(), "abcdef");
    }

    #[test]
    fn renders_text_input_cursor_using_display_width() {
        let view = View::TextInput(TextInputView {
            id:            Some(ViewId::from("input")),
            prompt:        vec![Span::new("> ", Role::Prompt)],
            value:         "a語b".to_owned(),
            placeholder:   None,
            cursor:        2,
            cursor_anchor: CursorAnchor::from("input/cursor"),
            error:         None,
        });

        let surface = render_surface(&view);

        assert_eq!(surface.plain_text(), "> a語b");
        assert_eq!(surface.cursor(), Some(screw::Position { row: 0, col: 5 }));
    }

    #[test]
    fn renders_calendar_like_core_plain_snapshot() {
        let view = View::Calendar(CalendarView {
            id:          Some(ViewId::from("calendar")),
            year:        2026,
            month:       6,
            month_label: "June 2026".to_owned(),
            weekdays:    vec!["Mo".to_owned(), "Tu".to_owned()],
            weeks:       vec![CalendarWeek {
                days: vec![
                    CalendarDay {
                        date:     bang_core::Date {
                            year:  2026,
                            month: 6,
                            day:   1,
                        },
                        label:    "1".to_owned(),
                        in_month: true,
                        selected: true,
                        today:    false,
                    },
                    CalendarDay {
                        date:     bang_core::Date {
                            year:  2026,
                            month: 6,
                            day:   2,
                        },
                        label:    "2".to_owned(),
                        in_month: true,
                        selected: false,
                        today:    true,
                    },
                ],
            }],
            selected:    bang_core::Date {
                year:  2026,
                month: 6,
                day:   1,
            },
            help:        vec![Span::new("enter submit", Role::Dim)],
        });

        let widget = BangView::new(view.clone());

        assert_eq!(render_plain(&widget), plain_snapshot(&view));
    }

    #[test]
    fn custom_theme_reaches_rendered_cells() {
        let view = View::Text(vec![Span::new("!", Role::Error)]);
        let theme = Theme::default().with(map_role(Role::Error), Style::default().fg(Color::Cyan));
        let surface = super::render_surface_with_theme(&view, theme);

        assert_eq!(
            surface.rows()[0].cells()[0].style,
            Style::default().fg(Color::Cyan)
        );
    }

    #[test]
    fn explicit_cursor_view_sets_current_row_cursor() {
        let view = View::Stack(vec![
            View::Line(vec![Span::normal("first")]),
            View::Line(vec![Span::normal("second")]),
            View::Cursor(CursorPlacement {
                anchor: CursorAnchor::from("manual"),
                column: 3,
            }),
        ]);

        let surface = render_surface(&view);

        assert_eq!(surface.cursor(), Some(screw::Position { row: 1, col: 3 }));
    }

    fn list_view_with_selection(selected: usize) -> View {
        View::List(ListView {
            id:       Some(ViewId::from("list")),
            header:   Vec::new(),
            rows:     ["alpha", "bravo", "charlie"]
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    ListRow {
                        id:       Some(ViewId::owned(format!("row/{index}"))),
                        spans:    vec![Span::new(
                            label,
                            if index == selected {
                                Role::Selected
                            } else {
                                Role::Normal
                            },
                        )],
                        value:    label.into(),
                        selected: index == selected,
                        checked:  None,
                    }
                })
                .collect(),
            selected: Some(selected),
            offset:   0,
            total:    3,
            help:     Vec::new(),
        })
    }

    fn items() -> Vec<SelectItem> {
        ["alpha", "bravo", "charlie", "delta", "echo"]
            .into_iter()
            .map(SelectItem::from)
            .collect()
    }

    fn text_input_view(value: &str, cursor: usize, error: Option<&str>) -> View {
        View::TextInput(TextInputView {
            id: Some(ViewId::from("input")),
            prompt: vec![Span::new("> ", Role::Prompt)],
            value: value.to_owned(),
            placeholder: Some("type".to_owned()),
            cursor,
            cursor_anchor: CursorAnchor::from("input/cursor"),
            error: error.map(str::to_owned),
        })
    }

    struct RecordingRetainedRenderer {
        renderer: RetainedRenderer<Vec<u8>>,
        stats:    Vec<RenderStats>,
    }

    impl RecordingRetainedRenderer {
        fn new(width: usize) -> Self {
            Self {
                renderer: RetainedRenderer::new(Vec::new()).width(width),
                stats:    Vec::new(),
            }
        }

        fn into_output(self) -> Vec<u8> {
            self.renderer.into_inner()
        }
    }

    impl bang_terminal::SessionRenderer for RecordingRetainedRenderer {
        fn render(&mut self, view: &View) -> io::Result<()> {
            self.stats.push(self.renderer.render(view)?);
            Ok(())
        }

        fn resize(&mut self, size: bang_terminal::TerminalSize) -> io::Result<()> {
            self.renderer.resize(usize::from(size.cols));
            Ok(())
        }
    }
}
