// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Context,
    Event,
    Key,
    KeyEvent,
    ListRow,
    ListView,
    Reaction,
    Role,
    Span,
    Value,
    View,
    ViewContext,
    ViewId,
    Widget,
    WidgetId,
};

const DEFAULT_PAGE_SIZE: usize = 9;

#[derive(Clone, Debug, PartialEq)]
pub struct SelectItem {
    pub label: String,
    pub value: Value,
}

impl SelectItem {
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
    wrap:      bool,
}

impl Select {
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
            page_size: DEFAULT_PAGE_SIZE,
            wrap:      true,
        }
    }

    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        self.ensure_visible();
        self
    }

    #[must_use]
    pub const fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    #[must_use]
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.set_header(header);
        self
    }

    pub fn set_header(&mut self, header: impl Into<String>) {
        self.header = vec![Span::new(header, Role::Prompt)];
    }

    #[must_use]
    pub fn with_header_spans(mut self, header: impl Into<Vec<Span>>) -> Self {
        self.set_header_spans(header);
        self
    }

    pub fn set_header_spans(&mut self, header: impl Into<Vec<Span>>) {
        self.header = header.into();
    }

    #[must_use]
    pub fn with_prompt(self, prompt: impl Into<String>) -> Self {
        self.with_header(prompt)
    }

    #[must_use]
    pub fn with_selected_index(mut self, selected: usize) -> Self {
        if !self.items.is_empty() {
            self.selected = selected.min(self.items.len() - 1);
            self.ensure_visible();
        }
        self
    }

    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    #[must_use]
    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.selected_index().map(|index| &self.items[index])
    }

    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
    }

    fn move_by(&mut self, delta: isize) -> Reaction {
        let Some(next) = move_index(self.selected, self.items.len(), delta, self.wrap) else {
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
        if self.items.is_empty() {
            return Reaction::Ignored;
        }
        let selected = selected.min(self.items.len() - 1);
        if selected == self.selected {
            return Reaction::Ignored;
        }
        self.selected = selected;
        self.ensure_visible();
        Reaction::Changed
    }

    fn ensure_visible(&mut self) {
        if self.items.is_empty() {
            self.selected = 0;
            self.top = 0;
            return;
        }

        self.selected = self.selected.min(self.items.len() - 1);
        let visible = self.visible_len();
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + visible {
            self.top = self.selected + 1 - visible;
        }

        let max_top = self.items.len().saturating_sub(visible);
        self.top = self.top.min(max_top);
    }

    fn visible_len(&self) -> usize {
        self.page_size.min(self.items.len()).max(1)
    }

    fn submit(&self) -> Reaction {
        self.selected_item().map_or(Reaction::Ignored, |item| {
            Reaction::Submit(item.value.clone())
        })
    }

    fn list_view(&self, checked: Option<&[bool]>) -> ListView {
        let visible = self.visible_len();
        let rows = self
            .items
            .iter()
            .enumerate()
            .skip(self.top)
            .take(visible)
            .map(|(index, item)| {
                let selected = Some(index) == self.selected_index();
                ListRow {
                    id: Some(ViewId::owned(format!("{}/row/{index}", self.id.as_str()))),
                    spans: vec![Span::new(
                        item.label.clone(),
                        if selected {
                            Role::Selected
                        } else {
                            Role::Normal
                        },
                    )],
                    value: item.value.clone(),
                    selected,
                    checked: checked.map(|values| values[index]),
                }
            })
            .collect();

        ListView {
            id: Some(ViewId::owned(format!("{}/list", self.id.as_str()))),
            header: self.header.clone(),
            rows,
            selected: self
                .selected_index()
                .map(|index| index.saturating_sub(self.top)),
            offset: self.top,
            total: self.items.len(),
            help: vec![Span::new("enter submit | esc cancel", Role::Dim)],
        }
    }
}

impl Widget for Select {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
        let Event::Key(key) = event else {
            return Reaction::Ignored;
        };

        match key.key {
            Key::Up => self.move_by(-1),
            Key::Down => self.move_by(1),
            Key::Home => self.move_to(0),
            Key::End => self.move_to(self.items.len().saturating_sub(1)),
            Key::PageUp => self.move_by(-visible_delta(self.visible_len())),
            Key::PageDown => self.move_by(visible_delta(self.visible_len())),
            Key::Char('k' | 'K') if no_modifiers(&key) => self.move_by(-1),
            Key::Char('j' | 'J') if no_modifiers(&key) => self.move_by(1),
            Key::Enter => self.submit(),
            Key::Esc => Reaction::Cancel,
            _ => Reaction::Ignored,
        }
    }

    fn view(&self, _cx: &ViewContext) -> View {
        View::List(self.list_view(None))
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
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        let select = Select::new(id, items);
        let checked = vec![false; select.items.len()];
        Self { select, checked }
    }

    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.select = self.select.with_page_size(page_size);
        self
    }

    #[must_use]
    pub fn with_wrap(mut self, wrap: bool) -> Self {
        self.select = self.select.with_wrap(wrap);
        self
    }

    #[must_use]
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.select = self.select.with_header(header);
        self
    }

    #[must_use]
    pub fn with_header_spans(mut self, header: impl Into<Vec<Span>>) -> Self {
        self.select = self.select.with_header_spans(header);
        self
    }

    #[must_use]
    pub fn with_prompt(self, prompt: impl Into<String>) -> Self {
        self.with_header(prompt)
    }

    #[must_use]
    pub fn with_selected_index(mut self, selected: usize) -> Self {
        self.select = self.select.with_selected_index(selected);
        self
    }

    #[must_use]
    pub fn with_checked_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        for index in indices {
            self.set_checked(index, true);
        }
        self
    }

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

    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.select.selected_index()
    }

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

    fn view(&self, _cx: &ViewContext) -> View {
        View::List(self.select.list_view(Some(&self.checked)))
    }

    fn current_value(&self) -> Option<Value> {
        Some(Value::List(self.checked_values()))
    }
}

fn move_index(current: usize, len: usize, delta: isize, wrap: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }

    let current = current.min(len - 1);
    if wrap {
        let len = isize::try_from(len).ok()?;
        let current = isize::try_from(current).ok()?;
        let next = (current + delta).rem_euclid(len);
        return usize::try_from(next).ok();
    }

    let next = current.saturating_add_signed(delta).min(len - 1);
    Some(next)
}

fn visible_delta(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

const fn no_modifiers(key: &KeyEvent) -> bool {
    key.modifiers.bits() == 0
}
