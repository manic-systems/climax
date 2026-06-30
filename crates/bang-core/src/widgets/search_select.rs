// SPDX-License-Identifier: EUPL-1.2

use super::{
    SelectItem,
    TextInput,
};
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
    TextInputView,
    View,
    ViewContext,
    ViewId,
    Widget,
    WidgetId,
};

const DEFAULT_PAGE_SIZE: usize = 9;

pub struct SearchSelect {
    id:        WidgetId,
    input:     TextInput,
    header:    Vec<Span>,
    items:     Vec<SelectItem>,
    matches:   Vec<usize>,
    selected:  usize,
    top:       usize,
    page_size: usize,
    wrap:      bool,
}

impl SearchSelect {
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        let id = id.into();
        let items: Vec<_> = items.into_iter().map(Into::into).collect();
        let matches = (0..items.len()).collect();
        Self {
            input: TextInput::new(WidgetId::owned(format!("{}/query", id.as_str())))
                .with_prompt("search: "),
            header: Vec::new(),
            id,
            items,
            matches,
            selected: 0,
            top: 0,
            page_size: DEFAULT_PAGE_SIZE,
            wrap: true,
        }
    }

    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.input.set_prompt(prompt);
        self
    }

    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.input = self.input.with_placeholder(placeholder);
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
    pub fn with_selected_match_index(mut self, selected: usize) -> Self {
        if !self.matches.is_empty() {
            self.selected = selected.min(self.matches.len() - 1);
            self.ensure_visible();
        }
        self
    }

    #[must_use]
    pub fn query(&self) -> &str {
        self.input.value()
    }

    #[must_use]
    pub fn matched_indices(&self) -> &[usize] {
        &self.matches
    }

    #[must_use]
    pub const fn selected_match_index(&self) -> Option<usize> {
        if self.matches.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    #[must_use]
    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.selected_match_index()
            .map(|selected| &self.items[self.matches[selected]])
    }

    fn handle_input(&mut self, event: Event, cx: &mut Context) -> Reaction {
        match self.input.handle(event, cx) {
            Reaction::Changed => {
                self.recompute_matches();
                Reaction::Changed
            },
            Reaction::Ignored | Reaction::Cancel | Reaction::Submit(_) | Reaction::Focus(_) => {
                Reaction::Ignored
            },
        }
    }

    fn recompute_matches(&mut self) {
        let query = self.query();
        self.matches = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| matches_query(&item.label, query).then_some(index))
            .collect();
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
        self.ensure_visible();
    }

    fn move_by(&mut self, delta: isize) -> Reaction {
        let Some(next) = move_index(self.selected, self.matches.len(), delta, self.wrap) else {
            return Reaction::Ignored;
        };
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        self.ensure_visible();
        Reaction::Changed
    }

    fn submit(&self) -> Reaction {
        self.selected_item().map_or(Reaction::Ignored, |item| {
            Reaction::Submit(item.value.clone())
        })
    }

    fn ensure_visible(&mut self) {
        if self.matches.is_empty() {
            self.selected = 0;
            self.top = 0;
            return;
        }

        self.selected = self.selected.min(self.matches.len() - 1);
        let visible = self.visible_len();
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + visible {
            self.top = self.selected + 1 - visible;
        }

        let max_top = self.matches.len().saturating_sub(visible);
        self.top = self.top.min(max_top);
    }

    fn visible_len(&self) -> usize {
        self.page_size.min(self.matches.len()).max(1)
    }

    fn input_view(&self) -> TextInputView {
        let View::TextInput(view) = self.input.view(&ViewContext::default()) else {
            unreachable!("TextInput must render TextInputView")
        };
        view
    }

    fn list_view(&self) -> ListView {
        let visible = self.visible_len();
        let rows = self
            .matches
            .iter()
            .enumerate()
            .skip(self.top)
            .take(visible)
            .map(|(match_index, item_index)| {
                let item = &self.items[*item_index];
                let selected = Some(match_index) == self.selected_match_index();
                ListRow {
                    id: Some(ViewId::owned(format!(
                        "{}/row/{}",
                        self.id.as_str(),
                        item_index
                    ))),
                    spans: highlight_match(&item.label, self.query(), selected),
                    value: item.value.clone(),
                    selected,
                    checked: None,
                }
            })
            .collect();

        ListView {
            id: Some(ViewId::owned(format!("{}/results", self.id.as_str()))),
            header: self.header.clone(),
            rows,
            selected: self
                .selected_match_index()
                .map(|selected| selected.saturating_sub(self.top)),
            offset: self.top,
            total: self.matches.len(),
            help: vec![Span::new(
                "type filter | enter submit | esc cancel",
                Role::Dim,
            )],
        }
    }
}

impl Widget for SearchSelect {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        match &event {
            Event::Key(key) => {
                match key.key {
                    Key::Up => self.move_by(-1),
                    Key::Down => self.move_by(1),
                    Key::PageUp => self.move_by(-visible_delta(self.visible_len())),
                    Key::PageDown => self.move_by(visible_delta(self.visible_len())),
                    Key::Char('k' | 'K') if no_modifiers(key) => self.move_by(-1),
                    Key::Char('j' | 'J') if no_modifiers(key) => self.move_by(1),
                    Key::Enter => self.submit(),
                    Key::Esc => Reaction::Cancel,
                    _ => self.handle_input(event, cx),
                }
            },
            Event::Paste(_) => self.handle_input(event, cx),
            Event::Resize { .. } | Event::Tick => Reaction::Ignored,
        }
    }

    fn view(&self, _cx: &ViewContext) -> View {
        View::Stack(vec![
            View::TextInput(self.input_view()),
            View::List(self.list_view()),
        ])
    }

    fn current_value(&self) -> Option<crate::Value> {
        self.selected_item().map(|item| item.value.clone())
    }
}

fn matches_query(label: &str, query: &str) -> bool {
    query.is_empty()
        || label
            .to_ascii_lowercase()
            .contains(&query.to_ascii_lowercase())
}

fn highlight_match(label: &str, query: &str, selected: bool) -> Vec<Span> {
    let base_role = if selected {
        Role::Selected
    } else {
        Role::Normal
    };
    if query.is_empty() {
        return vec![Span::new(label, base_role)];
    }

    let label_lower = label.to_ascii_lowercase();
    let query_lower = query.to_ascii_lowercase();
    let Some(start) = label_lower.find(&query_lower) else {
        return vec![Span::new(label, base_role)];
    };
    let end = start + query.len();

    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::new(&label[..start], base_role));
    }
    spans.push(Span::new(&label[start..end], Role::Match));
    if end < label.len() {
        spans.push(Span::new(&label[end..], base_role));
    }
    spans
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
