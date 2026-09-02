// SPDX-License-Identifier: EUPL-1.2

//! Screw renderer adapter for Bang's renderer-neutral view contract.
//!
//! This crate is integration infrastructure. Standalone Bang users normally
//! interact with the `bang` facade; renderer authors consume
//! [`bang_core::adapter`] and can use this crate as a reference implementation.

use std::{
    error, fmt,
    io::{self, IsTerminal as _, Write},
    sync::{Arc, Mutex},
};

use bang_core::{
    Value, Widget as BangWidget,
    adapter::{
        CalendarView, CursorPlacement, ListPresentation, ListRow, ListView, Presentation,
        Role as BangRole, Span, TextInputView, View,
    },
};
use bang_terminal::{RunOutcome, SessionRenderer, TerminalSize};
use screw::{
    CursorVisibility, LayoutMode, Position, RenderCtx, RenderStats, Renderer, Role as ScrewRole,
    Stack as ScrewStack, Style, Surface, Theme, VerticalViewport, Widget, widget,
};
use unicode_width::UnicodeWidthChar as _;

#[derive(Clone, Debug)]
pub struct BangView {
    view: View,
    presentation: Arc<Mutex<Presentation>>,
}

impl BangView {
    #[must_use]
    pub fn new(view: View) -> Self {
        Self {
            view,
            presentation: Arc::new(Mutex::new(Presentation { lists: Vec::new() })),
        }
    }

    #[must_use]
    pub const fn view(&self) -> &View {
        &self.view
    }

    #[must_use]
    pub fn into_view(self) -> View {
        self.view
    }

    #[must_use]
    pub fn presentation(&self) -> Presentation {
        self.presentation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl PartialEq for BangView {
    fn eq(&self, other: &Self) -> bool {
        self.view == other.view
    }
}

impl Widget for BangView {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.presentation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .lists
            .clear();
        let cursor = cursor_placement(&self.view).cloned();
        let mut renderer = ViewRenderer::new(ctx, out, self.presentation.clone(), cursor);
        renderer.render_view(&self.view);
    }
}

pub struct RetainedRenderer<W> {
    renderer: Renderer<W>,
    presentation: Presentation,
}

impl<W> RetainedRenderer<W>
where
    W: Write,
{
    pub const fn new(writer: W) -> Self {
        Self {
            renderer: Renderer::new(writer).cursor_visibility(CursorVisibility::FromSurface),
            presentation: Presentation { lists: Vec::new() },
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

    pub const fn resize_viewport(&mut self, width: usize, height: usize) {
        self.renderer.resize_viewport(width, height);
    }

    pub fn render(&mut self, view: &View) -> io::Result<RenderStats> {
        let widget = BangView::new(view.clone());
        let stats = self.renderer.draw(&widget)?;
        self.presentation = widget.presentation();
        Ok(stats)
    }

    #[must_use]
    pub const fn presentation(&self) -> &Presentation {
        &self.presentation
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
    fn render(&mut self, view: &View) -> io::Result<Presentation> {
        self.renderer.render(view)?;
        Ok(self.renderer.presentation().clone())
    }

    fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
        self.renderer
            .resize_viewport(usize::from(size.cols), usize::from(size.rows));
        Ok(())
    }
}

#[derive(Debug)]
pub enum LiveSessionError {
    Unavailable,
    RawMode(io::Error),
    Signals(io::Error),
    TerminalIo(io::Error),
    Cancelled,
    InputEnded,
    Signalled(i32),
    ReraiseSignal {
        signal: i32,
        source: io::Error,
    },
    Cleanup {
        primary: Option<Box<Self>>,
        failures: Vec<CleanupFailure>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CleanupStage {
    Renderer,
    Screen,
    Signals,
    RawMode,
}

#[derive(Debug)]
pub struct CleanupFailure {
    stage: CleanupStage,
    source: io::Error,
}

impl CleanupFailure {
    #[must_use]
    pub const fn stage(&self) -> CleanupStage {
        self.stage
    }

    #[must_use]
    pub const fn source_error(&self) -> &io::Error {
        &self.source
    }
}

impl LiveSessionError {
    #[must_use]
    pub fn primary(&self) -> Option<&Self> {
        match self {
            Self::Cleanup { primary, .. } => primary.as_deref(),
            _ => Some(self),
        }
    }

    #[must_use]
    pub fn cleanup_failures(&self) -> &[CleanupFailure] {
        match self {
            Self::Cleanup { failures, .. } => failures,
            _ => &[],
        }
    }
}

impl fmt::Display for LiveSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("interactive terminal input is unavailable"),
            Self::RawMode(error) => {
                if matches!(
                    error.kind(),
                    io::ErrorKind::Unsupported | io::ErrorKind::NotConnected
                ) {
                    write!(f, "{error}")
                } else {
                    write!(f, "failed to enable terminal raw mode: {error}")
                }
            },
            Self::Signals(error) => {
                write!(f, "failed to install terminal signal handlers: {error}")
            },
            Self::TerminalIo(error) => write!(f, "terminal I/O failed: {error}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::InputEnded => f.write_str("input ended before submit"),
            Self::Signalled(signal) => write!(f, "interrupted by signal {signal}"),
            Self::ReraiseSignal { signal, source } => {
                write!(f, "failed to re-raise signal {signal}: {source}")
            },
            Self::Cleanup { primary, failures } => {
                if let Some(primary) = primary {
                    write!(f, "{primary}; ")?;
                }
                f.write_str("terminal cleanup failed: ")?;
                for (index, failure) in failures.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    write!(f, "{:?}: {}", failure.stage, failure.source)?;
                }
                Ok(())
            },
        }
    }
}

impl error::Error for LiveSessionError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::RawMode(error)
            | Self::Signals(error)
            | Self::TerminalIo(error)
            | Self::ReraiseSignal { source: error, .. } => Some(error),
            Self::Cleanup { primary, failures } => primary
                .as_deref()
                .map(|error| error as &(dyn error::Error + 'static))
                .or_else(|| {
                    failures
                        .first()
                        .map(|failure| &failure.source as &(dyn error::Error + 'static))
                }),
            Self::Unavailable | Self::Cancelled | Self::InputEnded | Self::Signalled(_) => None,
        }
    }
}

/// run a session using `screw` for rendering
pub fn run_live_session(widget: impl BangWidget + 'static) -> Result<Value, LiveSessionError> {
    if !io::stdin().is_terminal()
        || !io::stderr().is_terminal()
        || std::env::var_os("TERM").is_some_and(|term| term == "dumb")
    {
        return Err(LiveSessionError::Unavailable);
    }
    run_live_session_forced(widget)
}

/// Run a session without first applying terminal capability policy.
pub fn run_live_session_forced(
    widget: impl BangWidget + 'static,
) -> Result<Value, LiveSessionError> {
    let stdin = io::stdin();
    let terminal = bang_terminal::TerminalModeGuard::activate(
        &stdin,
        bang_terminal::RawModeOptions::blocking(),
    )
    .map_err(LiveSessionError::RawMode)?;
    let mut signals = match bang_terminal::SignalGuard::install_terminal_handlers() {
        Ok(signals) => signals,
        Err(error) => {
            let mut failures = Vec::new();
            collect_cleanup(&mut failures, CleanupStage::RawMode, terminal.restore());
            return Err(with_cleanup(LiveSessionError::Signals(error), failures));
        },
    };
    let stderr = io::stderr();
    let mut stderr = stderr.lock();

    let mut screen = match bang_terminal::InlineScreenGuard::enter(&mut stderr) {
        Ok(screen) => screen,
        Err(error) => {
            let mut failures = Vec::new();
            collect_cleanup(&mut failures, CleanupStage::Signals, signals.restore());
            collect_cleanup(&mut failures, CleanupStage::RawMode, terminal.restore());
            return Err(with_cleanup(LiveSessionError::TerminalIo(error), failures));
        },
    };
    let mut renderer = ScrewSessionRenderer::new(screen.writer());
    let result = bang_terminal::drive_tty_session_with_signals(
        widget,
        stdin.lock(),
        &mut renderer,
        &mut signals,
    );
    let clear = renderer.clear();
    drop(renderer);
    let screen_cleanup = screen.leave();
    let signal_cleanup = signals.restore();
    let raw_cleanup = terminal.restore();

    let primary = match result {
        Ok(RunOutcome::Submitted(value)) => Ok(value),
        Ok(RunOutcome::Cancelled) => Err(LiveSessionError::Cancelled),
        Ok(RunOutcome::InputEnded) => Err(LiveSessionError::InputEnded),
        Ok(RunOutcome::Signalled(signal)) => Err(LiveSessionError::Signalled(signal)),
        Err(error) => Err(LiveSessionError::TerminalIo(error)),
    };
    let signal = primary.as_ref().err().and_then(|error| match error {
        LiveSessionError::Signalled(signal) => Some(*signal),
        _ => None,
    });
    let mut failures = Vec::new();
    collect_cleanup(&mut failures, CleanupStage::Renderer, clear.map(|_| ()));
    collect_cleanup(&mut failures, CleanupStage::Screen, screen_cleanup);
    collect_cleanup(&mut failures, CleanupStage::Signals, signal_cleanup);
    collect_cleanup(&mut failures, CleanupStage::RawMode, raw_cleanup);

    if let Some(signal) = signal {
        bang_terminal::restore_default_and_raise(signal)
            .map_err(|source| LiveSessionError::ReraiseSignal { signal, source })?;
    }
    if failures.is_empty() {
        primary
    } else {
        Err(LiveSessionError::Cleanup {
            primary: primary.err().map(Box::new),
            failures,
        })
    }
}

fn collect_cleanup(
    failures: &mut Vec<CleanupFailure>,
    stage: CleanupStage,
    result: io::Result<()>,
) {
    if let Err(source) = result {
        failures.push(CleanupFailure { stage, source });
    }
}

fn with_cleanup(primary: LiveSessionError, failures: Vec<CleanupFailure>) -> LiveSessionError {
    if failures.is_empty() {
        primary
    } else {
        LiveSessionError::Cleanup {
            primary: Some(Box::new(primary)),
            failures,
        }
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
    let context = RenderCtx::new()
        .with_layout_mode(LayoutMode::Clip)
        .with_theme(theme);
    let presentation = Arc::new(Mutex::new(Presentation::default()));
    let cursor = cursor_placement(view).cloned();
    let mut renderer = ViewRenderer::new(&context, out, presentation, cursor);
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
    context: &'a RenderCtx,
    out: &'a mut Surface,
    presentation: Arc<Mutex<Presentation>>,
    cursor: Option<CursorPlacement>,
    wrote_line: bool,
}

impl<'a> ViewRenderer<'a> {
    const fn new(
        context: &'a RenderCtx,
        out: &'a mut Surface,
        presentation: Arc<Mutex<Presentation>>,
        cursor: Option<CursorPlacement>,
    ) -> Self {
        Self {
            context,
            out,
            presentation,
            cursor,
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
                let children = children
                    .iter()
                    .filter(|view| !matches!(view, View::Cursor(_)))
                    .cloned()
                    .map(|view| {
                        widget(MappedView {
                            view,
                            presentation: self.presentation.clone(),
                            cursor: self.cursor.clone(),
                        })
                    })
                    .collect::<Vec<_>>();
                ScrewStack::new(children).render(self.context, self.out);
                self.wrote_line = true;
            },
            View::List(list) => self.render_list(list),
            View::TextInput(input) => self.render_text_input(input),
            View::Calendar(calendar) => self.render_calendar(calendar),
            View::Cursor(_) | View::Empty => {},
        }
    }

    fn render_list(&mut self, list: &ListView) {
        if !list.header.is_empty() {
            self.start_line();
            self.write_spans(&list.header);
        }
        if !list.rows.is_empty() || !list.help.is_empty() {
            self.start_line();
            let children = list
                .rows
                .iter()
                .cloned()
                .map(|row| widget(ListRowWidget(row)))
                .collect::<Vec<_>>();
            let mut viewport = VerticalViewport::new(children)
                .requested_start(list.requested_start)
                .anchor(list.selected)
                .max_children(list.max_visible);
            if !list.help.is_empty() {
                viewport = viewport.trailing(widget(SpansWidget(list.help.clone())));
            }
            let report = viewport.report_handle();
            viewport.render(self.context, self.out);
            if let Some(id) = &list.id {
                let report = report.report();
                self.presentation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .lists
                    .push(ListPresentation {
                        id: id.clone(),
                        visible: report.visible,
                        fully_visible: report.fully_visible,
                        page_up: report.page_up,
                        page_down: report.page_down,
                    });
            }
        }
    }

    fn render_text_input(&mut self, input: &TextInputView) {
        self.start_line();
        self.write_spans(&input.prompt);
        let prompt_width = self.out.current_col();
        let input_column = match &self.cursor {
            Some(cursor) if cursor.anchor == input.cursor_anchor => Some(cursor.column),
            Some(_) => None,
            None => Some(prefix_width(&input.value, input.cursor)),
        };

        if input.value.is_empty() {
            if let Some(placeholder) = &input.placeholder {
                self.write_text(placeholder, BangRole::Dim);
            }
        } else {
            self.write_text(&input.value, BangRole::Normal);
        }

        if let Some(input_column) = input_column {
            self.out.set_cursor(Position {
                row: self.out.height().saturating_sub(1),
                col: prompt_width.saturating_add(input_column),
            });
        }

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
        self.write_style_text(text, self.context.theme().style(map_role(role)));
    }

    fn write_style_text(&mut self, text: impl AsRef<str>, style: Style) {
        self.out.write(text, style);
    }
}

#[derive(Clone)]
struct MappedView {
    view: View,
    presentation: Arc<Mutex<Presentation>>,
    cursor: Option<CursorPlacement>,
}

impl Widget for MappedView {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        ViewRenderer::new(
            ctx,
            out,
            self.presentation.clone(),
            self.cursor.clone(),
        )
        .render_view(&self.view);
    }

    fn vertical_size(&self) -> screw::VerticalSize {
        match &self.view {
            View::List(_) => screw::VerticalSize::Flexible,
            View::Stack(children) if children.iter().any(view_is_flexible) => {
                screw::VerticalSize::Flexible
            },
            _ => screw::VerticalSize::Content,
        }
    }
}

fn view_is_flexible(view: &View) -> bool {
    match view {
        View::List(_) => true,
        View::Stack(children) => children.iter().any(view_is_flexible),
        _ => false,
    }
}

fn cursor_placement(view: &View) -> Option<&CursorPlacement> {
    match view {
        View::Cursor(cursor) => Some(cursor),
        View::Stack(children) => children.iter().filter_map(cursor_placement).next_back(),
        _ => None,
    }
}

struct ListRowWidget(ListRow);

impl Widget for ListRowWidget {
    fn render(&self, context: &RenderCtx, out: &mut Surface) {
        let row = &self.0;
        let marker_role = if row.selected {
            BangRole::Selected
        } else {
            BangRole::Dim
        };
        out.write(
            if row.selected { "> " } else { "  " },
            context.theme().style(map_role(marker_role)),
        );
        if let Some(checked) = row.checked {
            out.write(
                if checked { "[x] " } else { "[ ] " },
                context.theme().style(map_role(marker_role)),
            );
        }
        let continuation = if row.checked.is_some() {
            "      "
        } else {
            "  "
        };
        for span in &row.spans {
            let style = context.theme().style(map_role(span.role));
            for part in span.text.split_inclusive('\n') {
                let (text, newline) = part
                    .strip_suffix('\n')
                    .map_or((part, false), |text| (text, true));
                out.write(text, style);
                if newline {
                    out.newline();
                    out.write(continuation, context.theme().style(map_role(BangRole::Dim)));
                }
            }
        }
    }
}

struct SpansWidget(Vec<Span>);

impl Widget for SpansWidget {
    fn render(&self, context: &RenderCtx, out: &mut Surface) {
        for span in &self.0 {
            out.write(&span.text, context.theme().style(map_role(span.role)));
        }
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
    use bang_core::{
        CursorAnchor, Date, Event, Key, KeyEvent, Modifiers, Session, ViewId,
        adapter::{
            CalendarDay, CalendarView, CalendarWeek, CursorPlacement, ListRow, ListView, Role,
            Span, TextInputView, View, plain_snapshot,
        },
        widgets::Select,
    };
    use screw::{Color, Position, Role as ScrewRole, Style, Theme};

    use super::{RetainedRenderer, map_role, render_surface};

    fn date(day: u8) -> Date {
        Date::new(2026, 7, day).expect("test date is valid")
    }

    #[test]
    fn list_view_preserves_plain_content_and_semantic_styles() {
        let view = View::List(ListView {
            id: None,
            header: vec![Span::new("Choose", Role::Prompt)],
            rows: vec![
                ListRow {
                    id: None,
                    spans: vec![Span::normal("Alpha")],
                    selected: true,
                    checked: Some(true),
                },
                ListRow {
                    id: None,
                    spans: vec![Span::new("Beta", Role::Match)],
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

        let surface = render_surface(&view);
        assert_eq!(surface.plain_text(), plain_snapshot(&view));
        assert_eq!(
            surface.plain_text(),
            "Choose\n> [x] Alpha\n  [ ] Beta\nenter to select"
        );

        let theme = Theme::default();
        assert_eq!(
            surface.rows()[0].cells()[0].style,
            theme.style(ScrewRole::Prompt)
        );
        assert_eq!(
            surface.rows()[1].cells()[0].style,
            theme.style(ScrewRole::Selected)
        );
        assert_eq!(
            surface.rows()[2].cells()[6].style,
            theme.style(ScrewRole::Match)
        );
        assert_eq!(
            surface.rows()[3].cells()[0].style,
            theme.style(ScrewRole::Dim)
        );
    }

    #[test]
    fn list_view_indents_explicit_continuation_lines() {
        let view = View::List(ListView {
            id: None,
            header: Vec::new(),
            rows: vec![ListRow {
                id: None,
                spans: vec![Span::normal("first\n↳ second")],
                selected: true,
                checked: Some(false),
            }],
            selected: Some(0),
            requested_start: 0,
            total: 1,
            max_visible: None,
            help: Vec::new(),
        });

        assert_eq!(
            render_surface(&view).plain_text(),
            "> [ ] first\n      ↳ second"
        );
    }

    #[test]
    fn text_input_converts_scalar_cursor_to_display_column() {
        let view = View::TextInput(TextInputView {
            id: None,
            prompt: vec![Span::new("> ", Role::Prompt)],
            value: "a界z".to_owned(),
            placeholder: Some("unused".to_owned()),
            cursor: 2,
            cursor_anchor: CursorAnchor::borrowed("input"),
            error: Some("try again".to_owned()),
        });

        let surface = render_surface(&view);
        assert_eq!(surface.plain_text(), plain_snapshot(&view));
        assert_eq!(surface.plain_text(), "> a界z\ntry again");
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 5 }));
        assert_eq!(
            surface.rows()[1].cells()[0].style,
            Theme::default().style(ScrewRole::Error)
        );
    }

    #[test]
    fn cursor_view_resolves_a_text_input_anchor() {
        let anchor = CursorAnchor::borrowed("input");
        let view = View::Stack(vec![
            View::TextInput(TextInputView {
                id: None,
                prompt: vec![Span::new("> ", Role::Prompt)],
                value: "abcdef".to_owned(),
                placeholder: None,
                cursor: 0,
                cursor_anchor: anchor.clone(),
                error: None,
            }),
            View::Cursor(CursorPlacement { anchor, column: 4 }),
        ]);

        let surface = render_surface(&view);
        assert_eq!(surface.plain_text(), "> abcdef");
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 6 }));
    }

    #[test]
    fn retained_renderer_follows_bang_cursor_intent() {
        let input = View::TextInput(TextInputView {
            id: None,
            prompt: vec![Span::new("search: ", Role::Prompt)],
            value: String::new(),
            placeholder: Some("type to filter".to_owned()),
            cursor: 0,
            cursor_anchor: CursorAnchor::borrowed("search"),
            error: None,
        });
        let list = View::List(ListView {
            id: None,
            header: Vec::new(),
            rows: Vec::new(),
            selected: None,
            requested_start: 0,
            total: 0,
            max_visible: None,
            help: Vec::new(),
        });
        let mut renderer = RetainedRenderer::new(Vec::new());

        renderer.render(&input).unwrap();
        renderer.render(&list).unwrap();
        let output = renderer.into_inner();

        let show = output
            .windows(b"\x1b[?25h".len())
            .position(|part| part == b"\x1b[?25h")
            .expect("text input should show the terminal cursor");
        let hide = output
            .windows(b"\x1b[?25l".len())
            .position(|part| part == b"\x1b[?25l")
            .expect("cursorless view should hide the terminal cursor");
        assert!(show < hide);
    }

    #[test]
    fn physical_viewport_feedback_drives_page_navigation() {
        let select = Select::new(
            "choices",
            ["one", "two\ncontinued", "three", "four", "five"],
        )
        .with_header("choose");
        let mut session = Session::new(select);
        let _reaction = session.handle(Event::Resize { cols: 40, rows: 5 });
        let mut renderer = RetainedRenderer::new(Vec::new());
        renderer.resize_viewport(40, 5);

        renderer.render(&session.view()).unwrap();
        let first = renderer.presentation().clone();
        assert_eq!(first.lists[0].visible, 0..2);
        assert_eq!(first.lists[0].page_down, Some(2));
        session.set_presentation(first);

        renderer.resize_viewport(40, 7);
        renderer.render(&session.view()).unwrap();
        assert_eq!(renderer.presentation().lists[0].visible, 0..4);
        renderer.resize_viewport(40, 5);

        let reaction = session.handle(Event::Key(KeyEvent {
            key: Key::PageDown,
            modifiers: Modifiers::empty(),
        }));
        assert!(reaction.changed());
        renderer.render(&session.view()).unwrap();
        assert_eq!(renderer.presentation().lists[0].visible, 1..3);
        assert_eq!(renderer.presentation().lists[0].fully_visible, 1..3);
        let View::List(list) = session.view() else {
            panic!("select should render a list");
        };
        assert_eq!(list.selected, Some(2));
    }

    #[test]
    fn semantic_stack_shares_height_between_list_viewports() {
        let list = |id: &'static str, labels: [&str; 3]| {
            View::List(ListView {
                id: Some(ViewId::borrowed(id)),
                header: Vec::new(),
                rows: labels
                    .into_iter()
                    .map(|label| ListRow {
                        id: None,
                        spans: vec![Span::normal(label)],
                        selected: false,
                        checked: None,
                    })
                    .collect(),
                selected: Some(0),
                requested_start: 0,
                total: 3,
                max_visible: None,
                help: Vec::new(),
            })
        };
        let view = View::Stack(vec![
            View::Line(vec![Span::normal("header")]),
            list("first", ["one", "two", "three"]),
            list("second", ["four", "five", "six"]),
        ]);
        let mut renderer = RetainedRenderer::new(Vec::new());
        renderer.resize_viewport(40, 5);

        renderer.render(&view).unwrap();

        let presentation = renderer.presentation();
        assert_eq!(presentation.lists.len(), 2);
        assert_eq!(presentation.lists[0].visible, 0..2);
        assert_eq!(presentation.lists[1].visible, 0..2);
    }

    #[test]
    fn calendar_view_preserves_day_markers_and_roles() {
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
            help: vec![Span::new("arrows move", Role::Dim)],
        });

        let surface = render_surface(&view);
        assert_eq!(surface.plain_text(), plain_snapshot(&view));
        assert_eq!(
            surface.plain_text(),
            "July 2026\nMo Tu We\n> 1 * 2 . 3\narrows move"
        );
        let theme = Theme::default();
        assert_eq!(
            surface.rows()[2].cells()[0].style,
            theme.style(ScrewRole::Selected)
        );
        assert_eq!(
            surface.rows()[2].cells()[4].style,
            theme.style(ScrewRole::Success)
        );
        assert_eq!(
            surface.rows()[2].cells()[8].style,
            theme.style(ScrewRole::Dim)
        );
    }

    #[test]
    fn every_bang_role_has_an_explicit_screw_mapping() {
        let theme = Theme::default();
        let cases = [
            (Role::Prompt, ScrewRole::Prompt),
            (Role::Normal, ScrewRole::Normal),
            (Role::Dim, ScrewRole::Dim),
            (Role::Selected, ScrewRole::Selected),
            (Role::Match, ScrewRole::Match),
            (Role::Error, ScrewRole::Error),
            (Role::Success, ScrewRole::Success),
        ];

        for (bang, screw) in cases {
            assert_eq!(map_role(bang), screw);
            assert_eq!(theme.style(map_role(bang)), theme.style(screw));
        }

        // Keep the imports honest: concrete style data belongs to Screw, not
        // to the Bang adapter contract.
        assert_eq!(
            theme.style(ScrewRole::Error),
            Style::PLAIN.fg(Color::Red).bold()
        );
    }
}
