// SPDX-License-Identifier: EUPL-1.2

use std::collections::BTreeMap;

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

/// Where one row of a review stands.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReviewState {
    /// Not looked at yet.
    #[default]
    Unconfirmed,
    /// Accepted.
    Confirmed,
    /// Rejected, which hides the row when removed rows are hidden.
    Denied,
}

impl ReviewState {
    /// The lowercase name used in submitted output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unconfirmed => "unconfirmed",
            Self::Confirmed => "confirmed",
            Self::Denied => "denied",
        }
    }

    /// The state a press of space moves to, from unconfirmed to confirmed to
    /// denied and back.
    #[must_use]
    pub const fn cycle(self) -> Self {
        match self {
            Self::Unconfirmed => Self::Confirmed,
            Self::Confirmed => Self::Denied,
            Self::Denied => Self::Unconfirmed,
        }
    }

    /// The checkbox text drawn before a row in this state.
    #[must_use]
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Unconfirmed => "[*] ",
            Self::Confirmed => "[y] ",
            Self::Denied => "[x] ",
        }
    }

    /// The theme role the marker is drawn with.
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

/// Additional application-level action bound to one character key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewActionBinding {
    key:  char,
    name: String,
    help: String,
}

impl ReviewActionBinding {
    /// Bind `key` to the action `name`. The help text starts as the name.
    #[must_use]
    pub fn new(key: char, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            key,
            help: name.clone(),
            name,
        }
    }

    /// Set the text shown for this action in the key help.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// The character that triggers the action.
    #[must_use]
    pub const fn key(&self) -> char {
        self.key
    }

    /// The name the action submits.
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
    /// A review over `items`, every row unconfirmed and the first selected.
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
            page_size: navigation::DEFAULT_PAGE_SIZE,
            layout: PageLayout::default(),
            wrap: true,
            show_removed: true,
            output: ReviewOutput::Rows,
            custom_actions: Vec::new(),
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

    /// Choose whether denied rows are listed. Defaults to `true`.
    #[must_use]
    pub fn with_show_removed(mut self, show_removed: bool) -> Self {
        self.show_removed = show_removed;
        self.ensure_visible();
        self
    }

    /// Submit how the review ended and every row's state, rather than only the
    /// rows.
    #[must_use]
    pub const fn with_exit_output(mut self, exit_output: bool) -> Self {
        self.output = if exit_output {
            ReviewOutput::Exits { leave: true }
        } else {
            ReviewOutput::Rows
        };
        self
    }

    /// Choose whether leaving submits an exit result instead of cancelling.
    /// This only applies once exit output is on, and defaults to `true` there.
    #[must_use]
    pub const fn with_leave_output(mut self, leave_output: bool) -> Self {
        if matches!(self.output, ReviewOutput::Exits { .. }) {
            self.output = ReviewOutput::Exits {
                leave: leave_output,
            };
        }
        self
    }

    /// Replace the extra action keys. Any action turns on exit output.
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

    /// Start with the row at `selected`, clamped to the last row.
    #[must_use]
    pub fn with_selected_index(mut self, selected: usize) -> Self {
        if !self.items.is_empty() {
            self.selected = selected.min(self.items.len() - 1);
            self.ensure_visible();
        }
        self
    }

    /// Start each row in the matching state from `states`, which also becomes
    /// its initial state.
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

    /// The index of the selected row, or `None` when no row is visible.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.visible_iter()
            .any(|index| index == self.selected)
            .then_some(self.selected)
    }

    /// The state of the row at `index`, or `None` past the end.
    #[must_use]
    pub fn state(&self, index: usize) -> Option<ReviewState> {
        self.states.get(index).copied()
    }

    /// Whether denied rows are currently listed.
    #[must_use]
    pub const fn show_removed(&self) -> bool {
        self.show_removed
    }

    /// Set the state of the row at `index`. An index past the end is ignored.
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
        let Some(next) = move_index(current, visible.len(), delta, wrap) else {
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
        if let Some(action) = action {
            output.insert("action".to_owned(), Value::from(action));
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
            _ => Reaction::Ignored,
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
    fn no_row_is_selected_once_every_visible_row_is_removed() {
        let mut review = ReviewList::new("review", ["alpha", "beta"])
            .with_states([ReviewState::Denied, ReviewState::Denied])
            .with_show_removed(false);
        let mut cx = Context::new();
        assert_eq!(review.selected_index(), None);

        for key in ['y', 'u', 'x'] {
            assert_eq!(review.handle(Event::char(key), &mut cx), Reaction::Ignored);
        }
        assert_eq!(
            review.handle(Event::key(Key::Tab), &mut cx),
            Reaction::Ignored
        );
        assert_eq!(review.state(0), Some(ReviewState::Denied));
        assert_eq!(review.state(1), Some(ReviewState::Denied));
    }

    #[test]
    fn form_advances_on_review_submit_but_bubbles_action_keys() {
        use crate::widgets::Form;

        let review = || {
            ReviewList::new("review", ["alpha"])
                .with_exit_output(true)
                .with_custom_actions([ReviewActionBinding::new('g', "regenerate")])
        };

        let mut session = Session::new(
            Form::new("form")
                .with_field("review", review())
                .with_field("note", crate::widgets::TextInput::new("note")),
        );
        assert!(matches!(
            session.handle(Event::key(Key::Enter)),
            SessionReaction::Focus(_)
        ));
        assert_eq!(session.status(), &SessionStatus::Running);

        let mut session = Session::new(
            Form::new("form")
                .with_field("review", review())
                .with_field("note", crate::widgets::TextInput::new("note")),
        );
        match session.handle(Event::char('g')) {
            SessionReaction::Submit(Value::Object(output)) => {
                assert_eq!(output.get("exit").and_then(Value::as_str), Some("action"));
                assert_eq!(
                    output.get("action").and_then(Value::as_str),
                    Some("regenerate")
                );
            },
            other => panic!("review action should leave the form, got {other:?}"),
        }
        let SessionStatus::Submitted(Value::Object(output)) = session.status() else {
            panic!("form should submit the review action exit");
        };
        assert_eq!(output.get("exit").and_then(Value::as_str), Some("action"));
    }
}
