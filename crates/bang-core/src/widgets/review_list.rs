// SPDX-License-Identifier: EUPL-1.2

use std::collections::BTreeMap;

use super::SelectItem;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReviewState {
    #[default]
    Unconfirmed,
    Confirmed,
    Denied,
}

impl ReviewState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unconfirmed => "unconfirmed",
            Self::Confirmed => "confirmed",
            Self::Denied => "denied",
        }
    }

    #[must_use]
    pub const fn cycle(self) -> Self {
        match self {
            Self::Unconfirmed => Self::Confirmed,
            Self::Confirmed => Self::Denied,
            Self::Denied => Self::Unconfirmed,
        }
    }

    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Unconfirmed => "[*] ",
            Self::Confirmed => "[y] ",
            Self::Denied => "[x] ",
        }
    }

    #[must_use]
    pub const fn role(self) -> Role {
        match self {
            Self::Unconfirmed => Role::Match,
            Self::Confirmed => Role::Success,
            Self::Denied => Role::Error,
        }
    }
}

impl TryFrom<&str> for ReviewState {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "unconfirmed" => Ok(Self::Unconfirmed),
            "confirmed" => Ok(Self::Confirmed),
            "denied" => Ok(Self::Denied),
            _ => {
                Err(format!(
                    "invalid review state '{value}', expected unconfirmed, confirmed, or denied"
                ))
            },
        }
    }
}

/// Application-level action that can finish a review list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewAction {
    Save,
    Regen,
    Search,
    Add,
}

impl ReviewAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Save => "save",
            Self::Regen => "regen",
            Self::Search => "search",
            Self::Add => "add",
        }
    }
}

/// Additional application-level action bound to one character key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewActionBinding {
    key:  char,
    name: String,
    help: String,
}

impl ReviewActionBinding {
    #[must_use]
    pub fn new(key: char, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            key,
            help: name.clone(),
            name,
        }
    }

    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    #[must_use]
    pub const fn key(&self) -> char {
        self.key
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn help_text(&self) -> String {
        format!("{} {}", self.key, self.help)
    }
}

/// A list where each row keeps an independent confirm/deny/unset state.
#[derive(Clone, Debug)]
pub struct ReviewList {
    id:             WidgetId,
    header:         Vec<Span>,
    items:          Vec<SelectItem>,
    initial_states: Vec<ReviewState>,
    states:         Vec<ReviewState>,
    selected:       usize,
    top:            usize,
    page_size:      usize,
    wrap:           bool,
    show_removed:   bool,
    action_output:  bool,
    custom_actions: Vec<ReviewActionBinding>,
}

impl ReviewList {
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        let items: Vec<_> = items.into_iter().map(Into::into).collect();
        let states = vec![ReviewState::Unconfirmed; items.len()];
        Self {
            id: id.into(),
            header: Vec::new(),
            items,
            initial_states: states.clone(),
            states,
            selected: 0,
            top: 0,
            page_size: DEFAULT_PAGE_SIZE,
            wrap: true,
            show_removed: true,
            action_output: false,
            custom_actions: Vec::new(),
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
    pub fn with_show_removed(mut self, show_removed: bool) -> Self {
        self.show_removed = show_removed;
        self.ensure_visible();
        self
    }

    #[must_use]
    pub const fn with_action_output(mut self, action_output: bool) -> Self {
        self.action_output = action_output;
        self
    }

    #[must_use]
    pub fn with_custom_actions(
        mut self,
        actions: impl IntoIterator<Item = ReviewActionBinding>,
    ) -> Self {
        self.custom_actions = actions.into_iter().collect();
        self
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
    pub fn with_states(mut self, states: impl IntoIterator<Item = ReviewState>) -> Self {
        for (index, state) in states.into_iter().enumerate() {
            if let Some(slot) = self.states.get_mut(index) {
                *slot = state;
            }
            if let Some(slot) = self.initial_states.get_mut(index) {
                *slot = state;
            }
        }
        self.ensure_visible();
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
    pub fn state(&self, index: usize) -> Option<ReviewState> {
        self.states.get(index).copied()
    }

    #[must_use]
    pub const fn show_removed(&self) -> bool {
        self.show_removed
    }

    pub fn set_state(&mut self, index: usize, state: ReviewState) -> Reaction {
        let Some(slot) = self.states.get_mut(index) else {
            return Reaction::Ignored;
        };
        if *slot == state {
            return Reaction::Ignored;
        }
        *slot = state;
        Reaction::Changed
    }

    fn set_selected_state(&mut self, state: ReviewState) -> Reaction {
        let Some(index) = self.selected_index() else {
            return Reaction::Ignored;
        };
        self.set_state(index, state)
    }

    fn cycle_selected_state(&mut self) -> Reaction {
        let Some(index) = self.selected_index() else {
            return Reaction::Ignored;
        };
        self.set_state(index, self.states[index].cycle())
    }

    fn move_by(&mut self, delta: isize) -> Reaction {
        let visible = self.visible_indices();
        let Some(current) = visible.iter().position(|index| *index == self.selected) else {
            return Reaction::Ignored;
        };
        let Some(next) = move_index(current, visible.len(), delta, self.wrap) else {
            return Reaction::Ignored;
        };
        let next = visible[next];
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        self.ensure_visible();
        Reaction::Changed
    }

    fn move_to(&mut self, selected: usize) -> Reaction {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return Reaction::Ignored;
        }
        let selected = visible
            .iter()
            .copied()
            .find(|index| *index >= selected)
            .unwrap_or_else(|| *visible.last().expect("visible is not empty"));
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
        let visible_indices = self.visible_indices();
        if visible_indices.is_empty() {
            self.top = 0;
            return;
        }
        let selected_position = visible_indices
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or(0);
        self.selected = visible_indices[selected_position];

        let visible = self.visible_len();
        if selected_position < self.top {
            self.top = selected_position;
        } else if selected_position >= self.top + visible {
            self.top = selected_position + 1 - visible;
        }

        let max_top = visible_indices.len().saturating_sub(visible);
        self.top = self.top.min(max_top);
    }

    fn visible_len(&self) -> usize {
        self.page_size.min(self.visible_indices().len()).max(1)
    }

    fn visible_indices(&self) -> Vec<usize> {
        self.initial_states
            .iter()
            .enumerate()
            .filter(|(_index, state)| self.show_removed || **state != ReviewState::Denied)
            .map(|(index, _state)| index)
            .collect()
    }

    fn toggle_removed(&mut self) -> Reaction {
        self.show_removed = !self.show_removed;
        self.top = 0;
        self.ensure_visible();
        Reaction::Changed
    }

    fn output_rows(&self) -> Value {
        Value::List(
            self.items
                .iter()
                .zip(&self.states)
                .map(|(item, state)| {
                    Value::Object(BTreeMap::from([
                        ("label".to_owned(), Value::from(item.label.clone())),
                        ("value".to_owned(), item.value.clone()),
                        ("state".to_owned(), Value::from(state.as_str())),
                    ]))
                })
                .collect(),
        )
    }

    fn output_action(&self, action: ReviewAction) -> Value {
        self.output_action_name(action.as_str())
    }

    fn output_action_name(&self, action: &str) -> Value {
        Value::Object(BTreeMap::from([
            ("action".to_owned(), Value::from(action)),
            ("rows".to_owned(), self.output_rows()),
        ]))
    }

    fn submit(&self, action: ReviewAction) -> Reaction {
        if self.action_output {
            Reaction::Submit(self.output_action(action))
        } else {
            Reaction::Submit(self.output_rows())
        }
    }

    fn list_view(&self) -> ListView {
        let visible = self.visible_len();
        let visible_indices = self.visible_indices();
        let rows = self
            .visible_indices()
            .into_iter()
            .skip(self.top)
            .take(visible)
            .map(|index| {
                let item = &self.items[index];
                let state = self.states[index];
                let selected = Some(index) == self.selected_index();
                ListRow {
                    id: Some(ViewId::owned(format!("{}/row/{index}", self.id.as_str()))),
                    spans: vec![
                        Span::new(state.marker(), state.role()),
                        Span::new(
                            item.label.clone(),
                            if selected {
                                Role::Selected
                            } else {
                                Role::Normal
                            },
                        ),
                    ],
                    value: item.value.clone(),
                    selected,
                    checked: None,
                }
            })
            .collect();

        let help = if self.action_output {
            self.action_help()
        } else {
            "space cycle | y confirm | x deny | u unset | r removed | enter submit | esc cancel"
                .to_owned()
        };

        ListView {
            id: Some(ViewId::owned(format!("{}/list", self.id.as_str()))),
            header: self.header.clone(),
            rows,
            selected: self
                .selected_index()
                .and_then(|selected| visible_indices.iter().position(|index| *index == selected))
                .map(|position| position.saturating_sub(self.top)),
            offset: self.top,
            total: visible_indices.len(),
            help: vec![Span::new(help, Role::Dim)],
        }
    }

    fn action_help(&self) -> String {
        let mut parts = vec![
            "space cycle".to_owned(),
            "y confirm".to_owned(),
            "x deny".to_owned(),
            "u unset".to_owned(),
            "r removed".to_owned(),
            "enter save".to_owned(),
            "g regen".to_owned(),
            "s search".to_owned(),
            "a add".to_owned(),
        ];
        parts.extend(
            self.custom_actions
                .iter()
                .map(ReviewActionBinding::help_text),
        );
        parts.push("esc cancel".to_owned());
        parts.join(" | ")
    }
}

impl Widget for ReviewList {
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
            Key::Char(' ') | Key::Tab => self.cycle_selected_state(),
            Key::Char('r' | 'R') if no_modifiers(&key) => self.toggle_removed(),
            Key::Char('y' | 'Y' | 'c' | 'C') if no_modifiers(&key) => {
                self.set_selected_state(ReviewState::Confirmed)
            },
            Key::Char('x' | 'X' | 'n' | 'N') if no_modifiers(&key) => {
                self.set_selected_state(ReviewState::Denied)
            },
            Key::Char('u' | 'U') if no_modifiers(&key) => {
                self.set_selected_state(ReviewState::Unconfirmed)
            },
            Key::Char('g' | 'G') if no_modifiers(&key) && self.action_output => {
                Reaction::Submit(self.output_action(ReviewAction::Regen))
            },
            Key::Char('s' | 'S') if no_modifiers(&key) && self.action_output => {
                Reaction::Submit(self.output_action(ReviewAction::Search))
            },
            Key::Char('a' | 'A') if no_modifiers(&key) && self.action_output => {
                Reaction::Submit(self.output_action(ReviewAction::Add))
            },
            Key::Char(value) if no_modifiers(&key) && self.action_output => {
                self.custom_actions
                    .iter()
                    .find(|action| action.key == value)
                    .map_or(Reaction::Ignored, |action| {
                        Reaction::Submit(self.output_action_name(&action.name))
                    })
            },
            Key::Enter => self.submit(ReviewAction::Save),
            Key::Esc => Reaction::Cancel,
            _ => Reaction::Ignored,
        }
    }

    fn view(&self, _cx: &ViewContext) -> View {
        View::List(self.list_view())
    }

    fn current_value(&self) -> Option<Value> {
        Some(self.output_rows())
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
