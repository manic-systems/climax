// SPDX-License-Identifier: EUPL-1.2

use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    fmt,
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
    Align, LayoutMode, Role, Style, Surface, Theme, Viewport,
    measure::{expand_tabs, first_segment_end, split_padding},
    renderer::layout_surface,
    surface::append_surface,
    sync::lock,
    truncate, width,
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

/// How often a widget needs to be redrawn without being marked dirty.
///
/// A runtime combines the interests of the whole widget tree and never redraws faster than its
/// frame rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TickInterest {
    /// The widget only changes when the application marks the runtime dirty.
    Never,
    /// Redraw on every frame, as an animation does. [`RenderCtx::frame`] advances each time.
    EveryFrame,
    /// Redraw at most this often, for content such as a clock.
    Every(Duration),
}

/// Per-draw information passed to [`Widget::render`].
///
/// Widgets read it and never build one. Runtimes and tests build one with the `with_` methods.
#[derive(Clone, Copy, Debug)]
pub struct RenderCtx {
    frame: u64,
    columns: Option<usize>,
    rows: Option<usize>,
    layout_mode: LayoutMode,
    theme: Theme,
}

impl RenderCtx {
    /// Creates a context at frame zero with no size constraints, clipping and the default theme.
    pub const fn new() -> Self {
        Self {
            frame: 0,
            columns: None,
            rows: None,
            layout_mode: LayoutMode::Clip,
            theme: Theme::DEFAULT,
        }
    }

    /// Sets the frame counter.
    #[must_use]
    pub const fn with_frame(mut self, frame: u64) -> Self {
        self.frame = frame;
        self
    }

    /// Sets the columns and rows available to the widget, where `None` means unconstrained.
    #[must_use]
    pub const fn with_constraints(mut self, columns: Option<usize>, rows: Option<usize>) -> Self {
        self.columns = columns;
        self.rows = rows;
        self
    }

    /// Sets whether overlong rows are clipped or wrapped.
    #[must_use]
    pub const fn with_layout_mode(mut self, layout_mode: LayoutMode) -> Self {
        self.layout_mode = layout_mode;
        self
    }

    /// Sets the theme that resolves [`Role`] styles.
    #[must_use]
    pub const fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// A counter that advances on every draw, for animation.
    pub const fn frame(self) -> u64 {
        self.frame
    }

    /// Columns this widget may use, or `None` when the width is unknown.
    ///
    /// Widgets must handle `None` and not assume a terminal width.
    pub const fn available_columns(self) -> Option<usize> {
        self.columns
    }

    /// Rows this widget may use, or `None` when the height is unconstrained.
    pub const fn available_rows(self) -> Option<usize> {
        self.rows
    }

    /// The available columns and rows together, when both are known.
    pub const fn viewport(self) -> Option<Viewport> {
        match (self.columns, self.rows) {
            (Some(columns), Some(rows)) => Some(Viewport::new(columns, rows)),
            _ => None,
        }
    }

    /// Whether overlong rows are clipped or wrapped after rendering.
    pub const fn layout_mode(self) -> LayoutMode {
        self.layout_mode
    }

    /// The theme that resolves [`Role`] styles.
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

/// Something that can draw itself into a [`Surface`].
///
/// Implement this for your own types and pass them to a [`Runtime`](crate::Runtime) or a
/// [`Renderer`](crate::Renderer). See the
/// crate documentation for a complete example.
pub trait Widget {
    /// Writes the widget into `out`.
    ///
    /// Call [`Surface::write`] once or many times to add text in different styles. A newline in the
    /// text or a call to [`Surface::newline`] starts a new row, and a widget that is not the first
    /// on its row continues from the current column. Rendering must not block, and the widget is
    /// rendered again for every frame.
    fn render(&self, ctx: &RenderCtx, out: &mut Surface);

    /// Whether the widget changes on its own, which defaults to [`TickInterest::Never`].
    fn tick_interest(&self) -> TickInterest {
        TickInterest::Never
    }

    /// Describe how a vertical container should allocate height to this widget.
    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Content
    }
}

/// A cloneable widget reference suitable for sharing with a background
/// renderer.
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

/// A run of text in one style or role.
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
    /// Creates unstyled text.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            style: TextStyle::Concrete(Style::default()),
        }
    }

    /// Sets a concrete style, replacing any role.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = TextStyle::Concrete(style);
        self
    }

    /// Sets a role resolved through the active [`Theme`], replacing any style.
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

/// Cycles through a list of frames, one per draw, such as a spinner.
///
/// It asks for a redraw on every frame.
#[derive(Clone, Debug)]
pub struct Looping {
    frames: Arc<[String]>,
    style:  Style,
}

impl Looping {
    /// Creates a loop over `frames`.
    ///
    /// A loop with no frames renders nothing.
    pub fn new(frames: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Self {
            frames: frames.into_iter().map(|frame| frame.as_ref().to_owned()).collect(),
            style:  Style::default(),
        }
    }

    /// Sets the style of the frame.
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

/// The most recent lines pushed to it, such as the tail of a log.
///
/// Clones share the same lines, so one clone can be pushed to from another thread while a clone is
/// drawn. The runtime must be marked dirty after a push.
#[derive(Clone, Debug)]
pub struct WindowedLines {
    capacity: usize,
    lines:    Arc<Mutex<VecDeque<String>>>,
    style:    Style,
}

impl WindowedLines {
    /// Creates an empty window that keeps at most `capacity` lines.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            lines: Arc::new(Mutex::new(VecDeque::new())),
            style: Style::default(),
        }
    }

    /// Sets the style of every line.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Appends a line, discarding the oldest when the window is full.
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

    /// A copy of the lines currently held, oldest first.
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

/// A list of rows with one selected, scrolled to keep the selection visible.
#[derive(Clone, Debug)]
pub struct List {
    rows:          Arc<[String]>,
    selected:      usize,
    height:        usize,
    normal:        Role,
    selected_role: Role,
}

impl List {
    /// Creates a list showing every row with the first one selected.
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

    /// Selects the row at `selected`, clamped to the last row.
    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// Limits the number of visible rows.
    #[must_use]
    pub const fn height(mut self, height: usize) -> Self {
        self.height = height;
        self
    }

    /// Sets the roles of ordinary and selected rows.
    #[must_use]
    pub const fn roles(mut self, normal: Role, selected: Role) -> Self {
        self.normal = normal;
        self.selected_role = selected;
        self
    }

    /// Indices of the rows currently shown.
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

/// Rows of [`Span`] cells laid out in columns of a common display width.
///
/// Each column is as wide as its widest cell, measured with [`width`], and
/// cells are aligned within it. A cell containing newlines spans several rows.
/// When the columns do not fit the available width, the flexible columns
/// shrink, widest first, and their cells overflow as [`CellOverflow`] says. A column never
/// shrinks below one column, so a table with more columns than the width allows is clipped by the
/// renderer, and a cluster wider than its column becomes an ellipsis. The final cell of a row is
/// never padded on its trailing side. A table that follows other content on a row fits itself to
/// the columns left and indents its continuation rows to where it started. No cluster joins across
/// a cell boundary, so zero-width content at the start of a cell is dropped.
#[derive(Clone, Debug)]
pub struct Table {
    header: Option<Vec<String>>,
    header_style: Style,
    rows: Vec<Vec<Span>>,
    aligns: Vec<Align>,
    gap: usize,
    flexible: Option<Vec<usize>>,
    overflow: CellOverflow,
}

/// What a [`Table`] does with a cell wider than its shrunk column.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CellOverflow {
    /// Cut the cell and end it with an ellipsis.
    #[default]
    Truncate,
    /// Continue the cell on the following rows of its column.
    Wrap,
}

impl Table {
    /// Creates a table from rows of cells, where a cell is anything that
    /// converts into a [`Span`].
    pub fn new<R, C>(rows: R) -> Self
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = C>,
        C: Into<Span>,
    {
        Self {
            header: None,
            header_style: Style::new().bold(),
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().map(Into::into).collect())
                .collect(),
            aligns: Vec::new(),
            gap: 1,
            flexible: None,
            overflow: CellOverflow::Truncate,
        }
    }

    /// Adds a header row drawn above the others with the header style.
    #[must_use]
    pub fn header<I>(mut self, cells: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.header = Some(cells.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the style of the header row, which is bold by default.
    #[must_use]
    pub const fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the alignment of each column in order, leaving later columns
    /// left-aligned.
    #[must_use]
    pub fn aligns(mut self, aligns: impl IntoIterator<Item = Align>) -> Self {
        self.aligns = aligns.into_iter().collect();
        self
    }

    /// Sets the number of spaces between columns, which is one by default.
    #[must_use]
    pub const fn gap(mut self, gap: usize) -> Self {
        self.gap = gap;
        self
    }

    /// Limits shrinking to these columns when the table is too wide, instead
    /// of every column.
    #[must_use]
    pub fn flexible(mut self, columns: impl IntoIterator<Item = usize>) -> Self {
        self.flexible = Some(columns.into_iter().collect());
        self
    }

    /// Sets how a cell wider than its shrunk column overflows.
    #[must_use]
    pub const fn overflow(mut self, overflow: CellOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    fn fit(&self, widths: &mut [usize], available: usize) {
        let gaps = self.gap * widths.len().saturating_sub(1);
        let mut excess = (widths.iter().sum::<usize>() + gaps).saturating_sub(available);
        while excess > 0 {
            let widest = (0..widths.len())
                .filter(|column| self.flexible.as_ref().is_none_or(|flexible| flexible.contains(column)))
                .filter(|&column| widths[column] > 1)
                .max_by_key(|&column| widths[column]);
            let Some(column) = widest else {
                return;
            };
            widths[column] -= 1;
            excess -= 1;
        }
    }

    fn cell_lines(&self, value: &str, columns: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for line in value.split('\n') {
            let line = expand_tabs(line);
            let line = line.as_ref();
            if width(line) <= columns {
                lines.push(line.to_owned());
                continue;
            }
            match self.overflow {
                CellOverflow::Truncate => {
                    lines.push(format!("{}…", truncate(line, columns.saturating_sub(1))));
                },
                CellOverflow::Wrap => {
                    let mut rest = line;
                    while !rest.is_empty() {
                        let head = truncate(rest, columns);
                        if head.is_empty() {
                            lines.push(if columns > 0 { "…" } else { "" }.to_owned());
                            rest = &rest[first_segment_end(rest)..];
                        } else {
                            lines.push(head.to_owned());
                            rest = &rest[head.len()..];
                        }
                    }
                },
            }
        }
        lines
    }
}

impl Widget for Table {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let header: Option<Vec<Span>> = self.header.as_ref().map(|cells| {
            cells
                .iter()
                .map(|cell| Span::new(cell.as_str()).style(self.header_style))
                .collect()
        });
        let lines: Vec<&[Span]> = header
            .iter()
            .chain(&self.rows)
            .map(Vec::as_slice)
            .collect();

        let columns = lines
            .iter()
            .map(|line| {
                line.iter()
                    .rposition(|cell| !cell.text.value.is_empty())
                    .map_or(0, |last| last + 1)
            })
            .max()
            .unwrap_or(0);
        let mut widths: Vec<usize> = (0..columns)
            .map(|column| {
                lines
                    .iter()
                    .filter_map(|line| line.get(column))
                    .flat_map(|cell| cell.text.value.split('\n'))
                    .map(|line| width(&expand_tabs(line)))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let start = out.current_col();
        if let Some(available) = ctx.available_columns() {
            self.fit(&mut widths, available.saturating_sub(start));
        }

        let mut first = true;
        for line in &lines {
            let cells: Vec<Vec<String>> = line
                .iter()
                .enumerate()
                .take(columns)
                .map(|(column, cell)| self.cell_lines(&cell.text.value, widths[column]))
                .collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
            for row in 0..height {
                if !first {
                    out.newline();
                    out.write(" ".repeat(start), Style::new());
                }
                first = false;
                let filled = cells
                    .iter()
                    .rposition(|cell| cell.get(row).is_some_and(|text| !text.is_empty()))
                    .map_or(0, |column| column + 1);
                for (column, cell) in line.iter().enumerate().take(filled) {
                    if column > 0 {
                        out.write(" ".repeat(self.gap), Style::new());
                    }
                    let text = cells[column].get(row).map_or("", String::as_str);
                    let missing = widths[column].saturating_sub(width(text));
                    let align = self.aligns.get(column).copied().unwrap_or_default();
                    let (before, after) = split_padding(missing, align);
                    out.write(" ".repeat(before), Style::new());
                    out.seal();
                    Text {
                        value: text.to_owned(),
                        style: cell.text.style,
                    }
                    .render(ctx, out);
                    if column + 1 < filled {
                        out.write(" ".repeat(after), Style::new());
                    }
                }
            }
        }
    }
}

/// A bar of a fixed number of cells filled in proportion to a fraction.
///
/// Clones share the fraction, so one clone can be updated from another thread while a clone is
/// drawn. The runtime must be marked dirty after the fraction changes.
#[derive(Clone, Debug)]
pub struct ProgressBar {
    fraction: Arc<AtomicU32>,
    width:    usize,
    filled:   Style,
    empty:    Style,
}

impl ProgressBar {
    /// Creates an empty bar with `width` cells between its brackets.
    pub fn new(width: usize) -> Self {
        Self {
            fraction: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
            width,
            filled: Style::default(),
            empty: Style::default(),
        }
    }

    /// Sets the styles of the filled and the empty cells.
    #[must_use]
    pub const fn styles(mut self, filled: Style, empty: Style) -> Self {
        self.filled = filled;
        self.empty = empty;
        self
    }

    /// Sets how much of the bar is filled, clamped to the range 0 to 1.
    pub fn set_fraction(&self, fraction: f32) {
        self.fraction
            .store(fraction.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// How much of the bar is filled, from 0 to 1.
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

/// A prompt that places the terminal cursor after it, for input handled outside the renderer.
#[derive(Clone, Debug)]
pub struct InputAnchor {
    prompt: String,
    style:  Style,
}

impl InputAnchor {
    /// Creates an anchor that writes `prompt`.
    pub fn prompt(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            style:  Style::default(),
        }
    }

    /// Sets the style of the prompt.
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

/// A prompt followed by editable text with the terminal cursor placed inside it.
#[derive(Clone, Debug)]
pub struct TextInput {
    prompt:      String,
    value:       String,
    cursor:      usize,
    prompt_role: Role,
    value_role:  Role,
}

impl TextInput {
    /// Creates an input showing `prompt` and `value` with the cursor at the start.
    pub fn new(prompt: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            value:       value.into(),
            cursor:      0,
            prompt_role: Role::Prompt,
            value_role:  Role::Normal,
        }
    }

    /// Places the cursor before the character at index `cursor`, counted in characters and clamped
    /// to the end. An index inside a grapheme cluster moves to the end of that cluster.
    #[must_use]
    pub const fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    /// Sets the roles of the prompt and of the value.
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

/// Children written one after another on a single row.
#[derive(Clone)]
pub struct Line<H = WidgetRef> {
    children: Box<[H]>,
}

impl<H> fmt::Debug for Line<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Line")
            .field("children", &self.children.len())
            .finish()
    }
}

impl<H> Line<H> {
    /// Creates a line from `children`.
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

/// Children placed one below another, with flexible children sharing the height left over.
///
/// A child that is [`VerticalSize::Flexible`] receives the rows that content-sized siblings do not
/// use.
#[derive(Clone)]
pub struct Stack<H = WidgetRef> {
    children: Box<[H]>,
}

impl<H> fmt::Debug for Stack<H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Stack")
            .field("children", &self.children.len())
            .finish()
    }
}

impl<H> Stack<H> {
    /// Creates a stack from `children`.
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

/// Shows one of several widgets depending on a state that can change while it is drawn.
///
/// A state with no widget draws nothing.
pub struct Stateful<S, H = WidgetRef> {
    state: Mutex<S>,
    cases: HashMap<S, H>,
}

impl<S: fmt::Debug, H> fmt::Debug for Stateful<S, H> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Stateful")
            .field("state", &*lock(&self.state))
            .field("cases", &self.cases.len())
            .finish()
    }
}

impl<S, H> Stateful<S, H>
where
    S: Clone + Eq + Hash,
{
    /// Creates a switch in state `initial` with no cases.
    pub fn new(initial: S) -> Self {
        Self {
            state: Mutex::new(initial),
            cases: HashMap::new(),
        }
    }

    /// Shows `widget` while the state is `state`.
    #[must_use]
    pub fn case(mut self, state: S, widget: H) -> Self {
        self.cases.insert(state, widget);
        self
    }

    /// Changes the state, which takes effect on the next draw.
    pub fn set_state(&self, state: S) {
        *lock(&self.state) = state;
    }

    /// The current state.
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

impl Widget for &str {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(*self, Style::default());
    }
}

impl<T> Widget for &T
where
    T: Widget + ?Sized,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        (**self).render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        (**self).tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        (**self).vertical_size()
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

/// Combines the tick interests of several children into one for a composite
/// widget.
///
/// The result is [`TickInterest::EveryFrame`] if any child wants every frame,
/// otherwise the shortest [`TickInterest::Every`] interval, otherwise
/// [`TickInterest::Never`].
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
    fn table_pads_columns_to_a_common_display_width() {
        let table = Table::new([["name", "qty"], ["apple", "3"], ["fig", "12"]])
            .header(["item", "count"])
            .aligns([Align::Left, Align::Right])
            .gap(2);
        assert_eq!(
            render_plain(&table),
            "item   count\nname     qty\napple      3\nfig       12"
        );
    }

    #[test]
    fn table_measures_wide_cells_by_display_width() {
        let table = Table::new([["世界", "a"], ["ab", "b"]]).aligns([Align::Right]);
        assert_eq!(render_plain(&table), "世界 a\n  ab b");

        let mut surface = Surface::new();
        table.render(&RenderCtx::new(), &mut surface);
        assert_eq!(surface.row_width(0), surface.row_width(1));
    }

    #[test]
    fn table_styles_the_header_and_leaves_no_trailing_padding() {
        let table = Table::new([["a", "bb"]]).header(["xxx", "y"]).header_style(Style::new().underline());
        let mut surface = Surface::new();
        table.render(&RenderCtx::new(), &mut surface);
        assert_eq!(surface.plain_text(), "xxx y\na   bb");
        assert_eq!(surface.rows()[0].cells()[0].style, Style::new().underline());
        assert_eq!(surface.rows()[1].cells()[0].style, Style::new());
    }

    #[test]
    fn table_centres_cells_and_tolerates_ragged_rows() {
        let table = Table::new([vec!["ab", "x"], vec!["abcde"]]).aligns([Align::Center]);
        assert_eq!(render_plain(&table), " ab   x\nabcde");
        assert_eq!(render_plain(&Table::new(Vec::<Vec<Span>>::new())), "");
    }

    fn render_within(table: &Table, columns: usize) -> String {
        let mut surface = Surface::new();
        table.render(&RenderCtx::new().with_constraints(Some(columns), None), &mut surface);
        surface.plain_text()
    }

    #[test]
    fn table_spans_rows_for_multiline_cells() {
        let table = Table::new([["a\nbb", "x"], ["c", "y\nz"]]);
        assert_eq!(render_plain(&table), "a  x\nbb\nc  y\n   z");
    }

    #[test]
    fn table_shrinks_the_widest_column_to_fit() {
        let table = Table::new([["abcdefgh", "xy"]]);
        assert_eq!(render_within(&table, 8), "abcd… xy");
        assert_eq!(render_within(&table, 20), "abcdefgh xy");

        let wrapped = table.overflow(CellOverflow::Wrap);
        assert_eq!(render_within(&wrapped, 8), "abcde xy\nfgh");
    }

    #[test]
    fn table_shrinks_only_flexible_columns() {
        let table = Table::new([["abcdef", "ghijkl"]]).flexible([1]);
        assert_eq!(render_within(&table, 10), "abcdef gh…");
    }

    fn row_widths(surface: &Surface) -> Vec<usize> {
        (0..surface.height()).map(|row| surface.row_width(row)).collect()
    }

    #[test]
    fn table_wrap_replaces_a_cluster_wider_than_its_column() {
        let table = Table::new([["世界世界", "x"], ["ab", "y"]])
            .overflow(CellOverflow::Wrap)
            .flexible([0]);
        let mut surface = Surface::new();
        table.render(&RenderCtx::new().with_constraints(Some(3), None), &mut surface);
        assert_eq!(surface.plain_text(), "… x\n…\n…\n…\na y\nb");
        assert!(row_widths(&surface).iter().all(|&columns| columns <= 3));
    }

    #[test]
    fn table_cell_starting_with_zero_width_content_does_not_widen_the_previous_cell() {
        let table = Table::new([["❤", "\u{fe0f}", "B"]]).gap(0);
        let mut surface = Surface::new();
        table.render(&RenderCtx::new().with_constraints(Some(2), None), &mut surface);
        assert_eq!(surface.plain_text(), "❤B");
        assert_eq!(surface.display_width(), 2);
    }

    #[test]
    fn table_cell_ending_in_a_joiner_does_not_join_the_next_cell() {
        let table = Table::new([["👩\u{200d}", "💻", "B"], ["AA", "bb", "C"]]).gap(0);
        let mut surface = Surface::new();
        table.render(&RenderCtx::new().with_constraints(Some(5), None), &mut surface);
        assert_eq!(surface.plain_text(), "👩\u{200d}💻B\nAAbbC");
        assert_eq!(surface.rows()[0].cells().len(), 3);
        assert_eq!(surface.display_width(), 5);
    }

    #[test]
    fn table_wrap_ignores_a_dropped_control_before_a_wide_cluster() {
        let table = Table::new([["\u{1b}世世", "x"]])
            .overflow(CellOverflow::Wrap)
            .flexible([0]);
        let mut surface = Surface::new();
        table.render(&RenderCtx::new().with_constraints(Some(3), None), &mut surface);
        assert_eq!(surface.plain_text(), "… x\n…");
    }

    #[test]
    fn text_input_cursor_follows_tab_stops_from_the_prompt() {
        let mut surface = Surface::new();
        TextInput::new("> ", "a\tb").cursor(2).render(&RenderCtx::new(), &mut surface);
        assert_eq!(surface.cursor(), Some(crate::Position { row: 0, col: 8 }));
    }

    #[test]
    fn text_input_cursor_inside_a_cluster_moves_past_it() {
        let mut surface = Surface::new();
        TextInput::new("", "\u{2764}\u{fe0f}x")
            .cursor(1)
            .render(&RenderCtx::new(), &mut surface);
        assert_eq!(surface.cursor(), Some(crate::Position { row: 0, col: 2 }));
        assert_eq!(surface.plain_text(), "\u{2764}\u{fe0f}x");
    }

    #[test]
    fn table_keeps_emitted_rows_within_the_fitted_widths_for_clusters() {
        for overflow in [CellOverflow::Truncate, CellOverflow::Wrap] {
            let table = Table::new([
                ["\u{26a0}\u{fe0f}\u{26a0}\u{fe0f}\u{26a0}\u{fe0f}", "👩\u{200d}💻👩\u{200d}💻"],
                ["a\tb", "🇯🇵🇺🇸"],
            ])
            .overflow(overflow);
            for available in 2..14 {
                let mut surface = Surface::new();
                table.render(&RenderCtx::new().with_constraints(Some(available), None), &mut surface);
                let widest = row_widths(&surface).into_iter().max().unwrap_or(0);
                assert!(widest <= available.max(3), "{overflow:?} {available} {widest}");
            }
        }
    }

    #[test]
    fn table_ignores_trailing_columns_that_are_empty_in_every_row() {
        let table = Table::new([["long", ""]]);
        assert_eq!(render_within(&table, 4), "long");
    }

    #[test]
    fn table_honours_the_column_it_starts_at() {
        let table = Table::new([["a\nb", "c"]]);
        let line = Line::new(vec![widget("pre: "), widget(table)]);
        assert_eq!(render_plain(&line), "pre: a c\n     b");

        let mut surface = Surface::new();
        let wide = Table::new([["abcdefgh", "xy"]]);
        surface.write("pre: ", Style::new());
        wide.render(&RenderCtx::new().with_constraints(Some(13), None), &mut surface);
        assert_eq!(surface.plain_text(), "pre: abcd… xy");
    }

    #[test]
    fn borrowed_widgets_and_strings_render_without_a_static_lifetime() {
        let owned = String::from("owned");
        let borrowed: &str = &owned;
        assert_eq!(render_plain(&borrowed), "owned");

        let text = Text::new("text");
        let reference: &dyn Widget = &text;
        assert_eq!(render_plain(&reference), "text");
        assert_eq!(render_plain(&&text), "text");

        let stack = Stack::new(vec![&text as &dyn Widget, &"tail"]);
        assert_eq!(render_plain(&stack), "text\ntail");
    }

    #[test]
    fn looping_accepts_borrowed_arrays_vectors_and_iterators() {
        let frames: &[&str] = &["a", "b"];
        let render = |looping: Looping| {
            let mut surface = Surface::new();
            looping.render(&RenderCtx::new(), &mut surface);
            surface.plain_text()
        };
        assert_eq!(render(Looping::new(frames)), "a");
        assert_eq!(render(Looping::new(["a", "b"])), "a");
        assert_eq!(render(Looping::new(vec!["a".to_owned()])), "a");
        assert_eq!(render(Looping::new(frames.iter().copied())), "a");
        assert_eq!(render(Looping::new(Vec::<String>::new())), "");
    }

    #[test]
    fn reference_forwarding_keeps_tick_interest_and_vertical_size() {
        let looping = Looping::new(["a", "b"]);
        assert_eq!(looping.tick_interest(), TickInterest::EveryFrame);
        assert_eq!((&&"x").vertical_size(), VerticalSize::Content);
    }

    #[test]
    fn public_composite_types_implement_debug() {
        fn debuggable<T: fmt::Debug>(_: &T) {}
        debuggable(&Line::<WidgetRef>::new(Vec::new()));
        debuggable(&Stack::<WidgetRef>::new(Vec::new()));
        debuggable(&Stateful::<u8>::new(0));
        debuggable(&crate::layout());
        debuggable(&crate::VerticalViewport::<WidgetRef>::new(Vec::new()));
        debuggable(&crate::Layers::new("base"));
        debuggable(&crate::Renderer::new(Vec::new()));
        debuggable(&crate::Runtime::new(Vec::new(), "root"));
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
