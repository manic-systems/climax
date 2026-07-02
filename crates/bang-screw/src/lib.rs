// SPDX-License-Identifier: EUPL-1.2

//! adapter from bang views to screw render surface

use std::io::{self, Write};

use bang_core::{
    CalendarView, ListView, Role as BangRole, Span, TextInputView, Value, View,
    Widget as BangWidget,
};
use bang_terminal::{RunOutcome, SessionRenderer, TerminalSize};
use screw::{
    Position, RenderCtx, RenderStats, Renderer, Role as ScrewRole, Style, Surface, Theme, Widget,
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

/// `bang-terminal` session renderer backed by `screw`
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

/// run a session using `screw` for rendering
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
    theme: Theme,
    out: &'a mut Surface,
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
            // TODO allow customisation of indicators
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
