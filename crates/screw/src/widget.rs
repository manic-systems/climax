use std::{
    collections::{
        HashMap,
        VecDeque,
    },
    hash::Hash,
    sync::{
        Arc,
        Mutex,
    },
    time::Duration,
};

use unicode_width::UnicodeWidthChar as _;

use crate::{
    Role,
    Style,
    Surface,
    Theme,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TickInterest {
    Never,
    EveryFrame,
    Every(Duration),
}

#[derive(Clone, Copy, Debug)]
pub struct RenderCtx {
    pub frame: u64,
    pub width: Option<usize>,
    pub theme: Theme,
}

pub trait Widget: Send + Sync {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface);

    fn tick_interest(&self) -> TickInterest {
        TickInterest::Never
    }
}

pub type WidgetRef = Arc<dyn Widget>;

pub fn widget<W>(widget: W) -> WidgetRef
where
    W: Widget + 'static,
{
    Arc::new(widget)
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
            TextStyle::Role(role) => ctx.theme.style(role),
        };
        out.write(&self.value, style);
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
        let index = usize::try_from(ctx.frame % frame_count).unwrap_or(0);
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
        let mut lines = self.lines.lock().expect("windowed lines mutex poisoned");
        if self.capacity == 0 {
            return;
        }
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line.into());
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .expect("windowed lines mutex poisoned")
            .iter()
            .cloned()
            .collect()
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
            out.write(&self.rows[row_index], ctx.theme.style(role));
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
                out.write(&cell.text, ctx.theme.style(cell.role));
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProgressBar {
    fraction: Arc<Mutex<f32>>,
    width:    usize,
    filled:   Style,
    empty:    Style,
}

impl ProgressBar {
    pub fn new(width: usize) -> Self {
        Self {
            fraction: Arc::new(Mutex::new(0.0)),
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
        *self.fraction.lock().expect("progress bar mutex poisoned") = fraction.clamp(0.0, 1.0);
    }

    pub fn fraction(&self) -> f32 {
        *self.fraction.lock().expect("progress bar mutex poisoned")
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
        out.write(&self.prompt, ctx.theme.style(self.prompt_role));
        let prompt_width = out.current_col();
        let cursor = self.cursor.min(self.value.chars().count());
        let value_cursor_width: usize = self
            .value
            .chars()
            .take(cursor)
            .map(|ch| ch.width().unwrap_or(0))
            .sum();
        out.write(&self.value, ctx.theme.style(self.value_role));
        out.set_cursor(crate::Position {
            row: out.height().saturating_sub(1),
            col: prompt_width + value_cursor_width,
        });
    }
}

#[derive(Clone)]
pub struct Line {
    children: Arc<[WidgetRef]>,
}

impl Line {
    pub fn new(children: impl Into<Vec<WidgetRef>>) -> Self {
        Self {
            children: children.into().into(),
        }
    }
}

impl Widget for Line {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        for child in self.children.iter() {
            child.render(ctx, out);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(self.children.iter().map(Widget::tick_interest))
    }
}

#[derive(Clone)]
pub struct Stack {
    children: Arc<[WidgetRef]>,
}

impl Stack {
    pub fn new(children: impl Into<Vec<WidgetRef>>) -> Self {
        Self {
            children: children.into().into(),
        }
    }
}

impl Widget for Stack {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        for (index, child) in self.children.iter().enumerate() {
            if index > 0 {
                out.newline();
            }
            child.render(ctx, out);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(self.children.iter().map(Widget::tick_interest))
    }
}

pub struct Stateful<S> {
    state: Arc<Mutex<S>>,
    cases: HashMap<S, WidgetRef>,
}

impl<S> Stateful<S>
where
    S: Clone + Eq + Hash,
{
    pub fn new(initial: S) -> Self {
        Self {
            state: Arc::new(Mutex::new(initial)),
            cases: HashMap::new(),
        }
    }

    #[must_use]
    pub fn case(mut self, state: S, widget: WidgetRef) -> Self {
        self.cases.insert(state, widget);
        self
    }

    pub fn set_state(&self, state: S) {
        *self.state.lock().expect("stateful widget mutex poisoned") = state;
    }

    pub fn state(&self) -> S {
        self.state
            .lock()
            .expect("stateful widget mutex poisoned")
            .clone()
    }
}

impl<S> Widget for Stateful<S>
where
    S: Clone + Eq + Hash + Send + Sync,
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
