// SPDX-License-Identifier: EUPL-1.2

use screw::{
    RenderCtx,
    Role,
    Span,
    Surface,
    VerticalSize,
};

use super::navigation::{
    self,
    ListFrame,
    ListRow,
    PageLayout,
    move_index,
    no_modifiers,
    page_move,
};
use crate::{
    Context,
    Event,
    Key,
    Reaction,
    Value,
    Widget,
    WidgetId,
};

/// One choice, shown as `label` and submitted as `value`.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectItem {
    /// The text shown for the choice.
    pub label: String,
    /// The value submitted when the choice is picked.
    pub value: Value,
}

impl SelectItem {
    /// A choice shown as `label` that submits `value`.
    #[must_use]
    pub fn new(label: impl Into<String>, value: impl Into<Value>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

impl From<&str> for SelectItem {
    fn from(value: &str) -> Self {
        Self::new(value, value)
    }
}

impl From<String> for SelectItem {
    fn from(value: String) -> Self {
        Self::new(value.clone(), value)
    }
}

/// A single-choice list widget.
#[derive(Clone, Debug)]
pub struct Select {
    id:        WidgetId,
    header:    Vec<Span>,
    items:     Vec<SelectItem>,
    selected:  usize,
    top:       usize,
    page_size: usize,
    layout:    PageLayout,
    wrap:      bool,
}

impl Select {
    /// A list over `items` with the first choice selected.
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        Self {
            id:        id.into(),
            header:    Vec::new(),
            items:     items.into_iter().map(Into::into).collect(),
            selected:  0,
            top:       0,
            page_size: navigation::DEFAULT_PAGE_SIZE,
            layout:    PageLayout::default(),
            wrap:      true,
        }
    }

    /// Show at most `page_size` rows at once.
    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        self.ensure_visible();
        self
    }

    /// Choose whether moving past either end wraps around. Defaults to `true`.
    #[must_use]
    pub const fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Show `header` above the list.
    #[must_use]
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.set_header(header);
        self
    }

    /// Show `header` above the list.
    pub fn set_header(&mut self, header: impl Into<String>) {
        self.header = vec![Span::new(header).role(Role::Prompt)];
    }

    /// Show styled `header` spans above the list.
    #[must_use]
    pub fn with_header_spans(mut self, header: impl Into<Vec<Span>>) -> Self {
        self.set_header_spans(header);
        self
    }

    /// Show styled `header` spans above the list.
    pub fn set_header_spans(&mut self, header: impl Into<Vec<Span>>) {
        self.header = header.into();
    }

    /// Start with the choice at `selected`, clamped to the last choice.
    #[must_use]
    pub fn with_selected_index(mut self, selected: usize) -> Self {
        if !self.items.is_empty() {
            self.selected = selected.min(self.items.len() - 1);
            self.ensure_visible();
        }
        self
    }

    /// The index of the selected choice, or `None` for an empty list.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    /// The selected choice, or `None` for an empty list.
    #[must_use]
    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.selected_index().map(|index| &self.items[index])
    }

    /// The index of the first visible row.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
    }

    fn move_by(&mut self, delta: isize, wrap: bool) -> Reaction {
        let Some(next) = move_index(self.selected, self.items.len(), delta, wrap) else {
            return Reaction::Ignored;
        };
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        self.ensure_visible();
        Reaction::Changed
    }

    fn move_to(&mut self, selected: usize) -> Reaction {
        navigation::move_to(
            &mut self.selected,
            &mut self.top,
            selected,
            self.items.len(),
            self.page_size,
        )
    }

    fn move_page(&mut self, target: usize) -> Reaction {
        page_move(&mut self.top, &mut self.selected, target, self.items.len())
    }

    fn ensure_visible(&mut self) {
        navigation::ensure_visible(
            &mut self.selected,
            &mut self.top,
            self.items.len(),
            self.page_size,
        );
    }

    fn visible_len(&self) -> usize {
        navigation::visible_len(self.items.len(), self.page_size)
    }

    fn submit(&self) -> Reaction {
        self.selected_item().map_or(Reaction::Ignored, |item| {
            Reaction::Submit(item.value.clone())
        })
    }

    fn render_list(&self, checked: Option<&[bool]>, ctx: &RenderCtx, out: &mut Surface) {
        let window_size = self.page_size;
        let window = navigation::window_range(self.top, window_size, self.items.len());
        let rows = self.items[window.clone()]
            .iter()
            .enumerate()
            .map(|(offset, item)| {
                let index = window.start + offset;
                let selected = Some(index) == self.selected_index();
                ListRow {
                    spans: vec![(
                        item.label.clone(),
                        if selected {
                            Role::Selected
                        } else {
                            Role::Normal
                        },
                    )],
                    selected,
                    checked: checked.map(|values| values[index]),
                }
            })
            .collect();

        navigation::render_list(
            ListFrame {
                header: &self.header,
                rows,
                window_offset: window.start,
                selected: self.selected_index(),
                top: self.top,
                max_visible: self.page_size,
                help: "enter submit | esc cancel",
            },
            &self.layout,
            ctx,
            out,
        );
    }
}

impl screw::Widget for Select {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.render_list(None, ctx, out);
    }

    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Flexible
    }
}

impl Widget for Select {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
        self.layout.sync_top(&mut self.top);
        let Event::Key(key) = event else {
            return Reaction::Ignored;
        };

        match key.key {
            Key::Up => self.move_by(-1, self.wrap),
            Key::Down => self.move_by(1, self.wrap),
            Key::Home => self.move_to(0),
            Key::End => self.move_to(self.items.len().saturating_sub(1)),
            Key::PageUp => {
                match navigation::page_action(self.layout.target(false), self.visible_len(), false)
                {
                    navigation::PageAction::JumpTo(target) => self.move_page(target),
                    navigation::PageAction::ScrollBy(delta) => self.move_by(delta, false),
                }
            },
            Key::PageDown => {
                match navigation::page_action(self.layout.target(true), self.visible_len(), true) {
                    navigation::PageAction::JumpTo(target) => self.move_page(target),
                    navigation::PageAction::ScrollBy(delta) => self.move_by(delta, false),
                }
            },
            Key::Char('k' | 'K') if no_modifiers(&key) => self.move_by(-1, self.wrap),
            Key::Char('j' | 'J') if no_modifiers(&key) => self.move_by(1, self.wrap),
            Key::Enter => self.submit(),
            Key::Esc => Reaction::Cancel,
            _ => Reaction::Ignored,
        }
    }

    fn current_value(&self) -> Option<Value> {
        self.selected_item().map(|item| item.value.clone())
    }
}

/// A multiple-choice list widget.
#[derive(Clone, Debug)]
pub struct MultiSelect {
    select:  Select,
    checked: Vec<bool>,
}

impl MultiSelect {
    /// A list over `items` with nothing checked and the first choice selected.
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        let select = Select::new(id, items);
        let checked = vec![false; select.items.len()];
        Self { select, checked }
    }

    /// Show at most `page_size` rows at once.
    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.select = self.select.with_page_size(page_size);
        self
    }

    /// Choose whether moving past either end wraps around. Defaults to `true`.
    #[must_use]
    pub fn with_wrap(mut self, wrap: bool) -> Self {
        self.select = self.select.with_wrap(wrap);
        self
    }

    /// Show `header` above the list.
    #[must_use]
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.select = self.select.with_header(header);
        self
    }

    /// Show styled `header` spans above the list.
    #[must_use]
    pub fn with_header_spans(mut self, header: impl Into<Vec<Span>>) -> Self {
        self.select = self.select.with_header_spans(header);
        self
    }

    /// Start with the choice at `selected`, clamped to the last choice.
    #[must_use]
    pub fn with_selected_index(mut self, selected: usize) -> Self {
        self.select = self.select.with_selected_index(selected);
        self
    }

    /// Start with the choices at `indices` checked. Indices past the end are
    /// ignored.
    #[must_use]
    pub fn with_checked_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        for index in indices {
            self.set_checked(index, true);
        }
        self
    }

    /// The values of the checked choices in list order.
    #[must_use]
    pub fn checked_values(&self) -> Vec<Value> {
        self.select
            .items
            .iter()
            .zip(&self.checked)
            .filter(|(_item, checked)| **checked)
            .map(|(item, _checked)| item.value.clone())
            .collect()
    }

    /// The index of the highlighted choice, or `None` for an empty list.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.select.selected_index()
    }

    /// Check or uncheck the choice at `index`. An index past the end is
    /// ignored.
    pub fn set_checked(&mut self, index: usize, checked: bool) {
        if let Some(slot) = self.checked.get_mut(index) {
            *slot = checked;
        }
    }

    fn toggle_selected(&mut self) -> Reaction {
        let Some(index) = self.select.selected_index() else {
            return Reaction::Ignored;
        };
        self.checked[index] = !self.checked[index];
        Reaction::Changed
    }

    fn set_all(&mut self, checked: bool) -> Reaction {
        if self.checked.iter().all(|value| *value == checked) {
            return Reaction::Ignored;
        }
        self.checked.fill(checked);
        Reaction::Changed
    }
}

impl screw::Widget for MultiSelect {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.select.render_list(Some(&self.checked), ctx, out);
    }

    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Flexible
    }
}

impl Widget for MultiSelect {
    fn id(&self) -> WidgetId {
        self.select.id()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        let Event::Key(key) = &event else {
            return Reaction::Ignored;
        };

        match key.key {
            Key::Char(' ') | Key::Tab => self.toggle_selected(),
            Key::Char('a' | 'A') if no_modifiers(key) => self.set_all(true),
            Key::Char('n' | 'N') if no_modifiers(key) => self.set_all(false),
            Key::Enter => Reaction::Submit(Value::List(self.checked_values())),
            Key::Esc => Reaction::Cancel,
            _ => self.select.handle(event, cx),
        }
    }

    fn current_value(&self) -> Option<Value> {
        Some(Value::List(self.checked_values()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_built_stay_bounded_regardless_of_item_count() {
        let window = navigation::window_range(5_000, navigation::DEFAULT_PAGE_SIZE, 10_000);

        assert!(window.len() <= 3 * navigation::DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn rendering_shows_one_page_regardless_of_item_count() {
        let items: Vec<String> = (0..10_000).map(|index| index.to_string()).collect();
        let select = Select::new("s", items);

        let plain = screw::render_plain(&select);

        assert_eq!(plain.lines().count(), navigation::DEFAULT_PAGE_SIZE + 1);
    }
}
