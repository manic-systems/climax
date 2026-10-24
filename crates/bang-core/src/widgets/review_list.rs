// SPDX-License-Identifier: EUPL-1.2

use alloc::collections::BTreeMap;

use screw::{
    RenderCtx,
    Role,
    Span,
    Surface,
    VerticalSize,
};

use super::{
    SelectItem,
    navigation::{
        self,
        ListFrame,
        ListRow,
        PageLayout,
        move_index,
        no_modifiers,
    },
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

/// where one row of a review stands
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReviewState {
    /// not looked at yet
    #[default]
    Unconfirmed,
    /// accepted
    Confirmed,
    /// rejected, hides the row when removed rows are hidden
    Denied,
}

impl ReviewState {
    /// the lowercase name used in submitted output
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unconfirmed => "unconfirmed",
            Self::Confirmed => "confirmed",
            Self::Denied => "denied",
        }
    }

    /// the state a press of space moves to, from unconfirmed to confirmed to
    /// denied and back
    #[must_use]
    pub const fn cycle(self) -> Self {
        match self {
            Self::Unconfirmed => Self::Confirmed,
            Self::Confirmed => Self::Denied,
            Self::Denied => Self::Unconfirmed,
        }
    }

    /// the checkbox text drawn before a row in this state
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Unconfirmed => "[*] ",
            Self::Confirmed => "[y] ",
            Self::Denied => "[x] ",
        }
    }

    /// the theme role the marker is drawn with
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

/// extra application-level action bound to one character key
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewActionBinding {
    key:  char,
    name: String,
    help: String,
}

impl ReviewActionBinding {
    /// binds `key` to the action `name`, help starts as the name
    #[must_use]
    pub fn new(key: char, name: impl Into<String>) -> Self {
        let owned = name.into();
        Self {
            key,
            help: owned.clone(),
            name: owned,
        }
    }

    /// sets the text shown for this action in the key help
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// the character that triggers the action
    #[must_use]
    pub const fn key(&self) -> char {
        self.key
    }

    /// the name the action submits
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn help_text(&self) -> String {
        format!("{} {}", self.key, self.help)
    }
}

/// a list where each row keeps an independent confirm, deny or unset state
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
    layout:         PageLayout,
    wrap:           bool,
    show_removed:   bool,
    output:         ReviewOutput,
    custom_actions: Vec<ReviewActionBinding>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ReviewOutput {
    #[default]
    Rows,
    Exits {
        leave: bool,
    },
}

impl ReviewList {
    /// a review over `items`, every row unconfirmed and the first selected
    #[must_use]
    pub fn new<T>(id: impl Into<WidgetId>, items: impl IntoIterator<Item = T>) -> Self
    where
        T: Into<SelectItem>,
    {
        let collected: Vec<_> = items.into_iter().map(Into::into).collect();
        let states = vec![ReviewState::Unconfirmed; collected.len()];
        Self {
            id: id.into(),
            header: Vec::new(),
            items: collected,
            initial_states: states.clone(),
            states,
            selected: 0,
            top: 0,
            page_size: navigation::DEFAULT_PAGE_SIZE,
            layout: PageLayout::default(),
            wrap: true,
            show_removed: true,
            output: ReviewOutput::Rows,
            custom_actions: Vec::new(),
        }
    }

    /// shows at most `page_size` rows at once
    #[must_use]
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        self.ensure_visible();
        self
    }

    /// whether moving past either end wraps around, defaults to `true`
    #[must_use]
    pub const fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// shows `header` above the list
    #[must_use]
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.set_header(header);
        self
    }

    /// shows `header` above the list
    pub fn set_header(&mut self, header: impl Into<String>) {
        self.header = vec![Span::new(header).role(Role::Prompt)];
    }

    /// shows styled `header` spans above the list
    #[must_use]
    pub fn with_header_spans(mut self, header: impl Into<Vec<Span>>) -> Self {
        self.set_header_spans(header);
        self
    }

    /// shows styled `header` spans above the list
    pub fn set_header_spans(&mut self, header: impl Into<Vec<Span>>) {
        self.header = header.into();
    }

    /// whether rows that start out denied are listed, defaults to `true`
    #[must_use]
    pub fn with_show_removed(mut self, show_removed: bool) -> Self {
        self.show_removed = show_removed;
        self.ensure_visible();
        self
    }

    /// submits how the review ended and every row's state, instead of only the
    /// rows
    #[must_use]
    pub const fn with_exit_output(mut self, exit_output: bool) -> Self {
        self.output = if exit_output {
            ReviewOutput::Exits { leave: true }
        } else {
            ReviewOutput::Rows
        };
        self
    }

    /// whether leaving submits an exit result instead of cancelling, defaults
    /// to `true` with exit output
    #[must_use]
    pub const fn with_leave_output(mut self, leave_output: bool) -> Self {
        if matches!(self.output, ReviewOutput::Exits { .. }) {
            self.output = ReviewOutput::Exits {
                leave: leave_output,
            };
        }
        self
    }

    /// replaces the extra action keys, any action turns on exit output
    #[must_use]
    pub fn with_custom_actions(
        mut self,
        actions: impl IntoIterator<Item = ReviewActionBinding>,
    ) -> Self {
        self.custom_actions = actions.into_iter().collect();
        if !self.custom_actions.is_empty() && matches!(self.output, ReviewOutput::Rows) {
            self.output = ReviewOutput::Exits { leave: true };
        }
        self
    }

    /// starts with the row at `selected`, clamped to the last row
    #[must_use]
    pub fn with_selected_index(mut self, selected: usize) -> Self {
        if !self.items.is_empty() {
            self.selected = selected.min(self.items.len() - 1);
            self.ensure_visible();
        }
        self
    }

    /// starts each row in the matching state from `states`, which also becomes
    /// its initial state
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

    /// the index of the selected row, or `None` when no row is visible
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    /// the state of the row at `index`, or `None` past the end
    #[must_use]
    pub fn state(&self, index: usize) -> Option<ReviewState> {
        self.states.get(index).copied()
    }

    /// whether denied rows are currently listed
    #[must_use]
    pub const fn show_removed(&self) -> bool {
        self.show_removed
    }

    /// sets the state of the row at `index`, an index past the end is ignored
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

    fn move_by(&mut self, delta: isize, wrap: bool) -> Reaction {
        let visible = self.visible_indices();
        let Some(current) = visible.iter().position(|index| *index == self.selected) else {
            return Reaction::Ignored;
        };
        let Some(position) = move_index(current, visible.len(), delta, wrap) else {
            return Reaction::Ignored;
        };
        let next = visible[position];
        if next == self.selected {
            return Reaction::Ignored;
        }
        self.selected = next;
        self.ensure_visible();
        Reaction::Changed
    }

    fn move_to(&mut self, selected: usize) -> Reaction {
        let visible = self.visible_indices();
        let Some(&last) = visible.last() else {
            return Reaction::Ignored;
        };
        let landed = visible
            .iter()
            .copied()
            .find(|index| *index >= selected)
            .unwrap_or(last);
        if landed == self.selected {
            return Reaction::Ignored;
        }
        self.selected = landed;
        self.ensure_visible();
        Reaction::Changed
    }

    fn move_page(&mut self, target: usize) -> Reaction {
        let visible_indices = self.visible_indices();
        let Some(position) = visible_indices.iter().position(|index| *index == target) else {
            return Reaction::Ignored;
        };
        if target == self.selected && position == self.top {
            return Reaction::Ignored;
        }
        self.selected = target;
        self.top = position;
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
        let mut selected_position = visible_indices
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or(0);
        navigation::ensure_visible(
            &mut selected_position,
            &mut self.top,
            visible_indices.len(),
            self.page_size,
        );
        self.selected = visible_indices[selected_position];
    }

    fn page_target(&self, down: bool) -> Option<usize> {
        let position = self.layout.target(down)?;
        self.visible_iter().nth(position)
    }

    fn visible_len(&self) -> usize {
        navigation::visible_len(self.visible_iter().count(), self.page_size)
    }

    fn visible_iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.initial_states
            .iter()
            .enumerate()
            .filter(|(_index, state)| self.show_removed || **state != ReviewState::Denied)
            .map(|(index, _state)| index)
    }

    fn visible_indices(&self) -> Vec<usize> {
        self.visible_iter().collect()
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

    fn output_exit(&self, exit: &str, action: Option<&str>) -> Value {
        let mut output = BTreeMap::from([
            ("exit".to_owned(), Value::from(exit)),
            ("rows".to_owned(), self.output_rows()),
        ]);
        if let Some(name) = action {
            output.insert("action".to_owned(), Value::from(name));
        }
        Value::Object(output)
    }

    fn submit(&self) -> Reaction {
        if matches!(self.output, ReviewOutput::Exits { .. }) {
            Reaction::Submit(self.output_exit("submit", None))
        } else {
            Reaction::Submit(self.output_rows())
        }
    }

    fn leave(&self) -> Reaction {
        if self.output == (ReviewOutput::Exits { leave: true }) {
            Reaction::Action(self.output_exit("leave", None))
        } else {
            Reaction::Cancel
        }
    }

    fn action(&self, action: &str) -> Reaction {
        Reaction::Action(self.output_exit("action", Some(action)))
    }

    fn render_list(&self, ctx: &RenderCtx, out: &mut Surface) {
        let visible_indices = self.visible_indices();
        let window_size = self.page_size;
        let window = navigation::window_range(self.top, window_size, visible_indices.len());
        let rows = visible_indices[window.clone()]
            .iter()
            .copied()
            .map(|index| {
                let item = &self.items[index];
                let state = self.states[index];
                let selected = Some(index) == self.selected_index();
                ListRow {
                    spans: vec![
                        (state.marker().to_owned(), state.role()),
                        (
                            item.label.clone(),
                            if selected {
                                Role::Selected
                            } else {
                                Role::Normal
                            },
                        ),
                    ],
                    selected,
                    checked: None,
                }
            })
            .collect();

        navigation::render_list(
            ListFrame {
                header: &self.header,
                rows,
                window_offset: window.start,
                selected: self.selected_index().and_then(|selected| {
                    visible_indices.iter().position(|index| *index == selected)
                }),
                top: self.top,
                max_visible: self.page_size,
                help: &self.action_help(),
            },
            &self.layout,
            ctx,
            out,
        );
    }

    fn action_help(&self) -> String {
        let mut parts = vec![
            "space cycle".to_owned(),
            "y confirm".to_owned(),
            "x deny".to_owned(),
            "u unset".to_owned(),
            "r removed".to_owned(),
            "enter submit".to_owned(),
        ];
        if matches!(self.output, ReviewOutput::Exits { .. }) {
            parts.extend(
                self.custom_actions
                    .iter()
                    .map(ReviewActionBinding::help_text),
            );
        }
        if self.output == (ReviewOutput::Exits { leave: true }) {
            parts.push("esc leave".to_owned());
        } else {
            parts.push("esc cancel".to_owned());
        }
        parts.join(" | ")
    }
}

impl screw::Widget for ReviewList {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.render_list(ctx, out);
    }

    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Flexible
    }
}

impl Widget for ReviewList {
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
                match navigation::page_action(self.page_target(false), self.visible_len(), false) {
                    navigation::PageAction::JumpTo(target) => self.move_page(target),
                    navigation::PageAction::ScrollBy(delta) => self.move_by(delta, false),
                }
            },
            Key::PageDown => {
                match navigation::page_action(self.page_target(true), self.visible_len(), true) {
                    navigation::PageAction::JumpTo(target) => self.move_page(target),
                    navigation::PageAction::ScrollBy(delta) => self.move_by(delta, false),
                }
            },
            Key::Char('k' | 'K') if no_modifiers(&key) => self.move_by(-1, self.wrap),
            Key::Char('j' | 'J') if no_modifiers(&key) => self.move_by(1, self.wrap),
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
            Key::Char(value)
                if no_modifiers(&key) && matches!(self.output, ReviewOutput::Exits { .. }) =>
            {
                self.custom_actions
                    .iter()
                    .find(|action| action.key.eq_ignore_ascii_case(&value))
                    .map_or(Reaction::Ignored, |action| self.action(&action.name))
            },
            Key::Enter => self.submit(),
            Key::Esc => self.leave(),
            Key::Char(_)
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::Backtab
            | Key::Insert
            | Key::Function(_) => Reaction::Ignored,
        }
    }

    fn current_value(&self) -> Option<Value> {
        Some(self.output_rows())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Session,
        SessionReaction,
        SessionStatus,
    };

    #[test]
    fn structured_review_distinguishes_submit_leave_and_action() {
        let review = || {
            ReviewList::new("review", ["alpha"])
                .with_exit_output(true)
                .with_custom_actions([ReviewActionBinding::new('g', "regenerate")])
        };

        assert_exit(review(), Event::key(Key::Enter), "submit", None);
        assert_exit(review(), Event::key(Key::Esc), "leave", None);
        assert_exit(review(), Event::char('G'), "action", Some("regenerate"));
    }

    #[test]
    fn custom_actions_do_not_undo_an_earlier_leave_output_false() {
        let review = ReviewList::new("review", ["alpha"])
            .with_exit_output(true)
            .with_leave_output(false)
            .with_custom_actions([ReviewActionBinding::new('g', "regenerate")]);

        let mut session = Session::new(review);
        session.handle(Event::key(Key::Esc));
        assert_eq!(session.status(), &SessionStatus::Cancelled);
    }

    fn assert_exit(review: ReviewList, event: Event, exit: &str, action: Option<&str>) {
        let mut session = Session::new(review);
        session.handle(event);
        let SessionStatus::Submitted(Value::Object(output)) = session.status() else {
            panic!("structured review should submit an exit object");
        };
        assert_eq!(output.get("exit").and_then(Value::as_str), Some(exit));
        assert_eq!(output.get("action").and_then(Value::as_str), action);
        assert!(matches!(output.get("rows"), Some(Value::List(_))));
    }

    #[test]
    fn form_advances_on_review_submit_but_bubbles_action_keys() {
        use crate::widgets::{
            Form,
            TextInput,
        };

        let review = || {
            ReviewList::new("review", ["alpha"])
                .with_exit_output(true)
                .with_custom_actions([ReviewActionBinding::new('g', "regenerate")])
        };

        let mut session = Session::new(
            Form::new("form")
                .with_field("review", review())
                .with_field("note", TextInput::new("note")),
        );
        assert!(matches!(
            session.handle(Event::key(Key::Enter)),
            SessionReaction::Focus(_)
        ));
        assert_eq!(session.status(), &SessionStatus::Running);

        let mut action_session = Session::new(
            Form::new("form")
                .with_field("review", review())
                .with_field("note", TextInput::new("note")),
        );
        match action_session.handle(Event::char('g')) {
            SessionReaction::Submit(Value::Object(output)) => {
                assert_eq!(output.get("exit").and_then(Value::as_str), Some("action"));
                assert_eq!(
                    output.get("action").and_then(Value::as_str),
                    Some("regenerate")
                );
            },
            other @ (SessionReaction::Ignored
            | SessionReaction::Changed
            | SessionReaction::Cancel
            | SessionReaction::Focus(_)
            | SessionReaction::Submit(_)) => {
                panic!("review action should leave the form, got {other:?}")
            },
        }
        let SessionStatus::Submitted(Value::Object(output)) = action_session.status() else {
            panic!("form should submit the review action exit");
        };
        assert_eq!(output.get("exit").and_then(Value::as_str), Some("action"));
    }
}
