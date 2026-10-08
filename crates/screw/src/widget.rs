use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    hash::Hash,
    rc::Rc,
    sync::{
        Arc,
        Mutex,
        atomic::{
            AtomicU32,
            Ordering,
        },
    },
    time::Duration,
};

use crate::{
    LayoutMode, Role, Style, Surface, Theme, Viewport, renderer::layout_surface,
    surface::append_surface, sync::lock,
};

/// A widget's vertical allocation behavior inside a [`Stack`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VerticalSize {
    /// Measure the widget from its content before allocating flexible space.
    #[default]
    Content,
    /// Share the height left after content-sized siblings are measured.
    Flexible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TickInterest {
    Never,
    EveryFrame,
    Every(Duration),
}

#[derive(Clone, Copy, Debug)]
pub struct RenderCtx {
    frame: u64,
    columns: Option<usize>,
    rows: Option<usize>,
    layout_mode: LayoutMode,
    theme: Theme,
}

impl RenderCtx {
    pub const fn new() -> Self {
        Self {
            frame: 0,
            columns: None,
            rows: None,
            layout_mode: LayoutMode::Clip,
            theme: Theme::DEFAULT,
        }
    }

    #[must_use]
    pub const fn with_frame(mut self, frame: u64) -> Self {
        self.frame = frame;
        self
    }

    #[must_use]
    pub const fn with_constraints(mut self, columns: Option<usize>, rows: Option<usize>) -> Self {
        self.columns = columns;
        self.rows = rows;
        self
    }

    #[must_use]
    pub const fn with_layout_mode(mut self, layout_mode: LayoutMode) -> Self {
        self.layout_mode = layout_mode;
        self
    }

    #[must_use]
    pub const fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub const fn frame(self) -> u64 {
        self.frame
    }

    pub const fn available_columns(self) -> Option<usize> {
        self.columns
    }

    pub const fn available_rows(self) -> Option<usize> {
        self.rows
    }

    pub const fn viewport(self) -> Option<Viewport> {
        match (self.columns, self.rows) {
            (Some(columns), Some(rows)) => Some(Viewport::new(columns, rows)),
            _ => None,
        }
    }

    pub const fn layout_mode(self) -> LayoutMode {
        self.layout_mode
    }

    pub const fn theme(self) -> Theme {
        self.theme
    }

    pub(crate) const fn with_rows(mut self, rows: Option<usize>) -> Self {
        self.rows = rows;
        self
    }
}

impl Default for RenderCtx {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Widget {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface);

    fn tick_interest(&self) -> TickInterest {
        TickInterest::Never
    }

    /// Describe how a vertical container should allocate height to this widget.
    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Content
    }
}

pub type WidgetRef = Arc<dyn Widget + Send + Sync>;

/// A cloneable widget reference for composition and rendering on one thread.
pub type LocalWidgetRef<'a> = Rc<dyn Widget + 'a>;

/// Erase a thread-safe widget into a shared reference.
pub fn widget<W>(widget: W) -> WidgetRef
where
    W: Widget + Send + Sync + 'static,
{
    Arc::new(widget)
}

/// Erase a widget into a local reference, retaining any borrowed lifetime.
pub fn local_widget<'a, W>(widget: W) -> LocalWidgetRef<'a>
where
    W: Widget + 'a,
{
    Rc::new(widget)
}

#[derive(Clone, Debug)]
pub struct Text {
    value: String,
    style: TextStyle,
}

#[derive(Clone, Copy, Debug)]
enum TextStyle {
    Concrete(Style),
    Role(Role),
}

impl Text {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            style: TextStyle::Concrete(Style::default()),
        }
    }

    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = TextStyle::Concrete(style);
        self
    }

    #[must_use]
    pub const fn role(mut self, role: Role) -> Self {
        self.style = TextStyle::Role(role);
        self
    }
}

impl Widget for Text {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let style = match self.style {
            TextStyle::Concrete(style) => style,
            TextStyle::Role(role) => ctx.theme().style(role),
        };
        out.write(&self.value, style);
    }
}

/// A run of text with one style or role, meant to be combined in [`Spans`].
#[derive(Clone, Debug)]
pub struct Span {
    text: Text,
}

impl Span {
    /// Creates an unstyled span.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            text: Text::new(value),
        }
    }

    /// Sets a concrete style, replacing any role.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.text = self.text.style(style);
        self
    }

    /// Sets a role resolved through the active [`Theme`], replacing any style.
    #[must_use]
    pub fn role(mut self, role: Role) -> Self {
        self.text = self.text.role(role);
        self
    }
}

impl From<Text> for Span {
    fn from(text: Text) -> Self {
        Self { text }
    }
}

impl From<&str> for Span {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Span {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Several [`Span`]s written one after another, so a line can mix styles
/// without one widget allocation per span.
///
/// Spans are written exactly like [`Text`], so display width, clipping and
/// wrapping behave the same.
#[derive(Clone, Debug, Default)]
pub struct Spans {
    spans: Vec<Span>,
}

impl Spans {
    /// Creates an empty sequence.
    pub const fn empty() -> Self {
        Self { spans: Vec::new() }
    }

    /// Creates a sequence from anything convertible to spans, including
    /// [`Text`] and plain strings.
    pub fn new<I>(spans: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Span>,
    {
        spans.into_iter().collect()
    }

    /// Appends one span.
    #[must_use]
    pub fn push(mut self, span: impl Into<Span>) -> Self {
        self.spans.push(span.into());
        self
    }
}

impl<S: Into<Span>> FromIterator<S> for Spans {
    fn from_iter<I: IntoIterator<Item = S>>(spans: I) -> Self {
        Self {
            spans: spans.into_iter().map(Into::into).collect(),
        }
    }
}

impl Widget for Spans {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        for span in &self.spans {
            span.text.render(ctx, out);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Looping {
    frames: Arc<[String]>,
    style:  Style,
}

impl Looping {
    pub fn new<const N: usize>(frames: [&str; N]) -> Self {
        Self {
            frames: frames.map(ToOwned::to_owned).into(),
            style:  Style::default(),
        }
    }

    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Widget for Looping {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        if self.frames.is_empty() {
            return;
        }
        let frame_count = u64::try_from(self.frames.len()).unwrap_or(u64::MAX);
        let index = usize::try_from(ctx.frame() % frame_count).unwrap_or(0);
        out.write(&self.frames[index], self.style);
    }

    fn tick_interest(&self) -> TickInterest {
        TickInterest::EveryFrame
    }
}

#[derive(Clone, Debug)]
pub struct WindowedLines {
    capacity: usize,
    lines:    Arc<Mutex<VecDeque<String>>>,
    style:    Style,
}

impl WindowedLines {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            lines: Arc::new(Mutex::new(VecDeque::new())),
            style: Style::default(),
        }
    }

    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn push(&self, line: impl Into<String>) {
        if self.capacity == 0 {
            return;
        }
        let line = line.into();
        let mut lines = lock(&self.lines);
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    pub fn lines(&self) -> Vec<String> {
        lock(&self.lines).iter().cloned().collect()
    }
}

impl Widget for WindowedLines {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        for (index, line) in self.lines().iter().enumerate() {
            if index > 0 {
                out.newline();
            }
            out.write(line, self.style);
        }
    }
}

#[derive(Clone, Debug)]
pub struct List {
    rows:          Arc<[String]>,
    selected:      usize,
    height:        usize,
    normal:        Role,
    selected_role: Role,
}

impl List {
    pub fn new(rows: impl Into<Vec<String>>) -> Self {
        let rows = rows.into();
        let height = rows.len().max(1);
        Self {
            rows: rows.into(),
            selected: 0,
            height,
            normal: Role::Normal,
            selected_role: Role::Selected,
        }
    }

    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    #[must_use]
    pub const fn height(mut self, height: usize) -> Self {
        self.height = height;
        self
    }

    #[must_use]
    pub const fn roles(mut self, normal: Role, selected: Role) -> Self {
        self.normal = normal;
        self.selected_role = selected;
        self
    }

    pub fn visible_range(&self) -> std::ops::Range<usize> {
        if self.rows.is_empty() || self.height == 0 {
            return 0..0;
        }

        let selected = self.selected.min(self.rows.len() - 1);
        let height = self.height.min(self.rows.len());
        let start = selected
            .saturating_add(1)
            .saturating_sub(height)
            .min(self.rows.len() - height);
        start..start + height
    }
}

impl Widget for List {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let selected = self.selected.min(self.rows.len().saturating_sub(1));
        for (offset, row_index) in self.visible_range().enumerate() {
            if offset > 0 {
                out.newline();
            }
            let role = if row_index == selected {
                self.selected_role
            } else {
                self.normal
            };
            out.write(&self.rows[row_index], ctx.theme().style(role));
        }
    }
}

#[derive(Clone, Debug)]
pub struct Grid {
    rows: Arc<[Arc<[GridCell]>]>,
    gap:  usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GridCell {
    text: String,
    role: Role,
}

impl Grid {
    pub fn new(rows: impl Into<Vec<Vec<GridCell>>>) -> Self {
        Self {
            rows: rows
                .into()
                .into_iter()
                .map(|row| Arc::from(row.into_boxed_slice()))
                .collect::<Vec<_>>()
                .into(),
            gap:  1,
        }
    }

    #[must_use]
    pub const fn gap(mut self, gap: usize) -> Self {
        self.gap = gap;
        self
    }
}

impl GridCell {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            role: Role::Normal,
        }
    }

    #[must_use]
    pub const fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
}

impl Widget for Grid {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        for (row_index, row) in self.rows.iter().enumerate() {
            if row_index > 0 {
                out.newline();
            }
            for (cell_index, cell) in row.iter().enumerate() {
                if cell_index > 0 {
                    for _ in 0..self.gap {
                        out.write(" ", Style::default());
                    }
                }
                out.write(&cell.text, ctx.theme().style(cell.role));
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProgressBar {
    fraction: Arc<AtomicU32>,
    width:    usize,
    filled:   Style,
    empty:    Style,
}

impl ProgressBar {
    pub fn new(width: usize) -> Self {
        Self {
            fraction: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
            width,
            filled: Style::default(),
            empty: Style::default(),
        }
    }

    #[must_use]
    pub const fn styles(mut self, filled: Style, empty: Style) -> Self {
        self.filled = filled;
        self.empty = empty;
        self
    }

    pub fn set_fraction(&self, fraction: f32) {
        self.fraction
            .store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn fraction(&self) -> f32 {
        f32::from_bits(self.fraction.load(Ordering::Relaxed))
    }
}

impl Widget for ProgressBar {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        let filled = (self.fraction() * self.width as f32).round() as usize;
        out.write("[", Style::default());
        for _ in 0..filled.min(self.width) {
            out.write("━", self.filled);
        }
        for _ in filled.min(self.width)..self.width {
            out.write("─", self.empty);
        }
        out.write("]", Style::default());
    }
}

#[derive(Clone, Debug)]
pub struct InputAnchor {
    prompt: String,
    style:  Style,
}

impl InputAnchor {
    pub fn prompt(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            style:  Style::default(),
        }
    }

    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Widget for InputAnchor {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(&self.prompt, self.style);
        out.set_cursor_here();
    }
}

#[derive(Clone, Debug)]
pub struct TextInput {
    prompt:      String,
    value:       String,
    cursor:      usize,
    prompt_role: Role,
    value_role:  Role,
}

impl TextInput {
    pub fn new(prompt: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            value:       value.into(),
            cursor:      0,
            prompt_role: Role::Prompt,
            value_role:  Role::Normal,
        }
    }

    #[must_use]
    pub const fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    #[must_use]
    pub const fn roles(mut self, prompt: Role, value: Role) -> Self {
        self.prompt_role = prompt;
        self.value_role = value;
        self
    }
}

impl Widget for TextInput {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        use unicode_segmentation::UnicodeSegmentation as _;

        let value_style = ctx.theme().style(self.value_role);
        out.write(&self.prompt, ctx.theme().style(self.prompt_role));
        let char_index = self
            .value
            .char_indices()
            .nth(self.cursor)
            .map_or(self.value.len(), |(index, _)| index);
        let split = self
            .value
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain([self.value.len()])
            .find(|&index| index >= char_index)
            .unwrap_or(self.value.len());
        let (before, after) = self.value.split_at(split);
        out.write(before, value_style);
        out.set_cursor_here();
        out.write(after, value_style);
    }
}

#[derive(Clone)]
pub struct Line<H = WidgetRef> {
    children: Box<[H]>,
}

impl<H> Line<H> {
    pub fn new(children: impl Into<Vec<H>>) -> Self {
        Self {
            children: children.into().into_boxed_slice(),
        }
    }
}

impl<H> Widget for Line<H>
where
    H: Widget,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        for child in &self.children {
            child.render(ctx, out);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(self.children.iter().map(Widget::tick_interest))
    }

    fn vertical_size(&self) -> VerticalSize {
        combine_vertical_size(self.children.iter())
    }
}

#[derive(Clone)]
pub struct Stack<H = WidgetRef> {
    children: Box<[H]>,
}

impl<H> Stack<H> {
    pub fn new(children: impl Into<Vec<H>>) -> Self {
        Self {
            children: children.into().into_boxed_slice(),
        }
    }
}

impl<H> Widget for Stack<H>
where
    H: Widget,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let flexible = self
            .children
            .iter()
            .filter(|child| child.vertical_size() == VerticalSize::Flexible)
            .count();
        if ctx.available_rows().is_none() || flexible == 0 {
            self.render_sequentially(ctx, out);
            return;
        }

        let mut rendered = vec![None; self.children.len()];
        let mut fixed_height = 0_usize;
        for (index, child) in self.children.iter().enumerate() {
            if child.vertical_size() == VerticalSize::Content {
                let mut surface = Surface::new();
                child.render(&ctx.with_rows(None), &mut surface);
                let surface = layout_surface(surface, ctx.available_columns(), ctx.layout_mode());
                fixed_height = fixed_height.saturating_add(surface.height());
                let height = surface.height();
                rendered[index] = Some((surface, height));
            }
        }

        let available = ctx
            .available_rows()
            .unwrap_or_default()
            .saturating_sub(out.height().saturating_sub(1))
            .saturating_sub(fixed_height);
        let each = available / flexible;
        let mut extra = available % flexible;
        let mut first = true;
        for (index, child) in self.children.iter().enumerate() {
            let (surface, limit) = rendered[index].take().unwrap_or_else(|| {
                let allocation = each + usize::from(extra > 0);
                extra = extra.saturating_sub(1);
                let mut surface = Surface::new();
                child.render(&ctx.with_rows(Some(allocation)), &mut surface);
                (
                    layout_surface(surface, ctx.available_columns(), ctx.layout_mode()),
                    allocation,
                )
            });
            if limit == 0 {
                continue;
            }
            if !first {
                out.newline();
            }
            append_surface(out, &surface, limit);
            first = false;
        }
    }

    fn vertical_size(&self) -> VerticalSize {
        combine_vertical_size(self.children.iter())
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(self.children.iter().map(Widget::tick_interest))
    }
}

impl<H> Stack<H>
where
    H: Widget,
{
    fn render_sequentially(&self, ctx: &RenderCtx, out: &mut Surface) {
        for (index, child) in self.children.iter().enumerate() {
            if index > 0 {
                out.newline();
            }
            child.render(ctx, out);
        }
    }
}

pub struct Stateful<S, H = WidgetRef> {
    state: Mutex<S>,
    cases: HashMap<S, H>,
}

impl<S, H> Stateful<S, H>
where
    S: Clone + Eq + Hash,
{
    pub fn new(initial: S) -> Self {
        Self {
            state: Mutex::new(initial),
            cases: HashMap::new(),
        }
    }

    #[must_use]
    pub fn case(mut self, state: S, widget: H) -> Self {
        self.cases.insert(state, widget);
        self
    }

    pub fn set_state(&self, state: S) {
        *lock(&self.state) = state;
    }

    pub fn state(&self) -> S {
        lock(&self.state).clone()
    }
}

impl<S, H> Widget for Stateful<S, H>
where
    S: Clone + Eq + Hash,
    H: Widget,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        if let Some(widget) = self.cases.get(&self.state()) {
            widget.render(ctx, out);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        self.cases
            .get(&self.state())
            .map_or(TickInterest::Never, Widget::tick_interest)
    }

    fn vertical_size(&self) -> VerticalSize {
        self.cases
            .get(&self.state())
            .map_or(VerticalSize::Content, Widget::vertical_size)
    }
}

impl<T> Widget for Arc<T>
where
    T: Widget + ?Sized,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.as_ref().render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.as_ref().tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.as_ref().vertical_size()
    }
}

impl<T> Widget for Rc<T>
where
    T: Widget + ?Sized,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.as_ref().render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.as_ref().tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.as_ref().vertical_size()
    }
}

impl<T> Widget for Box<T>
where
    T: Widget + ?Sized,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.as_ref().render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.as_ref().tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.as_ref().vertical_size()
    }
}

impl Widget for String {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(self, Style::default());
    }
}

impl Widget for &'static str {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(*self, Style::default());
    }
}

fn combine_vertical_size<'a, H: Widget + 'a>(
    children: impl IntoIterator<Item = &'a H>,
) -> VerticalSize {
    if children
        .into_iter()
        .any(|child| child.vertical_size() == VerticalSize::Flexible)
    {
        VerticalSize::Flexible
    } else {
        VerticalSize::Content
    }
}

pub fn combine_tick_interest(interests: impl IntoIterator<Item = TickInterest>) -> TickInterest {
    let mut every: Option<Duration> = None;
    for interest in interests {
        match interest {
            TickInterest::EveryFrame => return TickInterest::EveryFrame,
            TickInterest::Every(duration) => {
                every = Some(every.map_or(duration, |current| current.min(duration)));
            },
            TickInterest::Never => {},
        }
    }
    every.map_or(TickInterest::Never, TickInterest::Every)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_plain;

    #[test]
    fn spans_render_back_to_back_with_their_own_styles() {
        let bold = Style::new().bold();
        let red = Style::new().fg(crate::Color::Red);
        let spans = Spans::new([
            Span::new("ab").style(bold),
            Span::new("cd"),
            Span::new("ef").style(red),
        ]);
        let mut surface = Surface::new();
        spans.render(&RenderCtx::new(), &mut surface);

        let styles: Vec<Style> = surface.rows()[0]
            .cells()
            .iter()
            .map(|cell| cell.style)
            .collect();
        assert_eq!(styles, [bold, bold, Style::new(), Style::new(), red, red]);
        assert_eq!(surface.plain_text(), "abcdef");
    }

    #[test]
    fn roles_resolve_through_the_context_theme() {
        let theme = Theme::DEFAULT.with(Role::Error, Style::new().underline());
        let spans = Spans::new([Span::new("bad").role(Role::Error)]);
        let mut surface = Surface::new();
        spans.render(&RenderCtx::new().with_theme(theme), &mut surface);
        assert_eq!(surface.rows()[0].cells()[0].style, Style::new().underline());
    }

    #[test]
    fn text_and_strings_convert_into_spans() {
        let spans = Spans::new(["a"]).push(String::from("b")).push(Text::new("c").role(Role::Dim));
        assert_eq!(render_plain(&spans), "abc");
        assert_eq!(render_plain(&Spans::empty()), "");
        let from_text: Spans = [Text::new("x"), Text::new("y")].into_iter().collect();
        assert_eq!(render_plain(&from_text), "xy");
    }

    #[test]
    fn spans_wrap_and_clip_like_the_equivalent_text() {
        let spans = Spans::new([Span::new("ab").style(Style::new().bold()), Span::new("世界cd")]);
        let text = Text::new("ab世界cd");

        let laid_out = |widget: &dyn Widget, width, mode| {
            let mut surface = Surface::new();
            widget.render(&RenderCtx::new(), &mut surface);
            layout_surface(surface, Some(width), mode).plain_text()
        };
        for mode in [LayoutMode::Wrap, LayoutMode::Clip] {
            for width in [3, 4, 5] {
                assert_eq!(
                    laid_out(&spans, width, mode),
                    laid_out(&text, width, mode),
                    "{mode:?} at {width}"
                );
            }
        }
    }
}
