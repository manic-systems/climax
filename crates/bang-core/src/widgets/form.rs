// SPDX-License-Identifier: EUPL-1.2

use std::collections::BTreeMap;

use screw::{
    LocalWidgetRef, RenderCtx, Role, Span, Spans, Stack, Surface, Text, TickInterest,
    VerticalSize, combine_tick_interest, local_widget,
};

use crate::{Context, Event, FocusTarget, Key, Reaction, Value, Widget, WidgetId};

/// Several named widgets shown together, with one active at a time.
/// Submitting the form yields an object with each field's value under its name.
///
/// A field that accepted a value is not asked again until it changes. Key and
/// paste events always count as changes. On any other event the field counts as
/// changed when its `current_value` differs from the accepted value. A widget
/// without a `current_value` can only signal a change through input.
pub struct Form {
    id:     WidgetId,
    fields: Vec<FormField>,
    active: usize,
}

impl Form {
    /// An empty form.
    #[must_use]
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id:     id.into(),
            fields: Vec::new(),
            active: 0,
        }
    }

    /// Add a field called `name` that edits through `widget`.
    #[must_use]
    pub fn with_field(mut self, name: impl Into<String>, widget: impl Widget + 'static) -> Self {
        self.push_field(name, widget);
        self
    }

    /// Add a field called `name` that edits through `widget`.
    ///
    /// # Panics
    ///
    /// Panics when a field called `name` already exists, since the submitted
    /// object holds one value per name and would drop the earlier answer.
    pub fn push_field(&mut self, name: impl Into<String>, widget: impl Widget + 'static) {
        let name = name.into();
        assert!(
            self.fields.iter().all(|field| field.name != name),
            "form field name '{name}' is already used"
        );
        self.fields.push(FormField {
            name,
            widget:   Box::new(widget),
            accepted: None,
        });
        self.active = self.active.min(self.fields.len().saturating_sub(1));
    }

    /// How many fields the form has.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the form has no fields.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The index of the field that receives input.
    #[must_use]
    pub const fn active_index(&self) -> usize {
        self.active
    }

    /// The id of the active field's widget, or `None` for an empty form.
    #[must_use]
    pub fn active_widget_id(&self) -> Option<WidgetId> {
        self.fields.get(self.active).map(|field| field.widget.id())
    }

    /// Make the field at `active` receive input, clamped to the last field.
    pub fn set_active_index(&mut self, active: usize) -> Reaction {
        if self.fields.is_empty() {
            self.active = 0;
            return Reaction::Ignored;
        }

        let active = active.min(self.fields.len() - 1);
        if active == self.active {
            return Reaction::Ignored;
        }
        self.active = active;
        self.focus_reaction()
    }

    fn move_focus(&mut self, delta: isize) -> Reaction {
        if self.fields.is_empty() {
            return Reaction::Ignored;
        }
        let len = self.fields.len();
        let next = self.active.saturating_add_signed(delta).min(len - 1);
        self.set_active_index(next)
    }

    fn focus_reaction(&self) -> Reaction {
        self.active_widget_id().map_or(Reaction::Changed, |id| {
            Reaction::Focus(FocusTarget::Widget(id))
        })
    }

    fn submit_or_advance(&mut self, accepted: Value) -> Reaction {
        if self.fields.is_empty() {
            return Reaction::Submit(self.object_value());
        }
        self.fields[self.active].accepted = Some(accepted);
        if self.active + 1 == self.fields.len() {
            return self.submit_all();
        }
        self.move_focus(1)
    }

    /// Ask every field that has no accepted value to accept one by sending it
    /// Enter, so a validator that Tab skipped still runs. Fields already
    /// accepted and not edited since keep their value and are not asked again.
    /// Focus lands on the first field that refuses.
    fn submit_all(&mut self) -> Reaction {
        let mut values = BTreeMap::new();
        for index in 0..self.fields.len() {
            let value = if let Some(value) = self.fields[index].accepted.clone() {
                value
            } else {
                let mut cx = Context::new();
                match self.fields[index].widget.handle(Event::key(Key::Enter), &mut cx) {
                    Reaction::Submit(value) => {
                        self.fields[index].accepted = Some(value.clone());
                        value
                    },
                    Reaction::Action(value) => return Reaction::Action(value),
                    Reaction::Cancel => return Reaction::Cancel,
                    _ => return self.refuse(index),
                }
            };
            values.insert(self.fields[index].name.clone(), value);
        }
        Reaction::Submit(Value::Object(values))
    }

    fn refuse(&mut self, index: usize) -> Reaction {
        if index == self.active {
            return Reaction::Changed;
        }
        self.active = index;
        self.focus_reaction()
    }

    fn object_value(&self) -> Value {
        Value::Object(
            self.fields
                .iter()
                .map(|field| {
                    (
                        field.name.clone(),
                        field
                            .widget
                            .current_value()
                            .or_else(|| field.accepted.clone())
                            .unwrap_or(Value::Null),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        )
    }
}

impl screw::Widget for Form {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let mut children: Vec<LocalWidgetRef<'_>> = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            let active = index == self.active;
            children.push(local_widget(Spans::new([
                Span::new(if active { "> " } else { "  " }).role(Role::Dim),
                Span::new(field.name.clone()).role(if active {
                    Role::Selected
                } else {
                    Role::Dim
                }),
            ])));
            children.push(local_widget(FieldWidget {
                widget: &*field.widget,
                focused: active,
            }));
        }

        if !self.fields.is_empty() {
            children.push(local_widget(
                Text::new("tab next | shift-tab previous | enter accept | esc cancel")
                    .role(Role::Dim),
            ));
        }

        Stack::new(children).render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(self.fields.iter().map(|field| field.widget.tick_interest()))
    }

    fn vertical_size(&self) -> VerticalSize {
        if self
            .fields
            .iter()
            .any(|field| field.widget.vertical_size() == VerticalSize::Flexible)
        {
            VerticalSize::Flexible
        } else {
            VerticalSize::Content
        }
    }
}

impl Widget for Form {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        match &event {
            Event::Key(key) => {
                match key.key {
                    Key::Tab => return self.move_focus(1),
                    Key::Backtab => return self.move_focus(-1),
                    Key::Esc => return Reaction::Cancel,
                    _ => {},
                }
            },
            Event::Resize { .. } | Event::Tick | Event::Paste(_) | Event::UnknownEscape(_) => {},
        }

        let Some(field) = self.fields.get_mut(self.active) else {
            return match event {
                Event::Key(key) if key.key == Key::Enter => Reaction::Submit(self.object_value()),
                _ => Reaction::Ignored,
            };
        };

        let edits = matches!(event, Event::Key(_) | Event::Paste(_));
        let reaction = field.widget.handle(event, cx);
        if !matches!(reaction, Reaction::Submit(_) | Reaction::Ignored) {
            let moved = field
                .widget
                .current_value()
                .is_some_and(|value| field.accepted.as_ref() != Some(&value));
            if edits || moved {
                field.accepted = None;
            }
        }
        match reaction {
            Reaction::Action(value) => Reaction::Action(value),
            Reaction::Submit(value) => self.submit_or_advance(value),
            Reaction::Cancel => Reaction::Cancel,
            Reaction::Focus(FocusTarget::Next) => self.move_focus(1),
            Reaction::Focus(FocusTarget::Previous) => self.move_focus(-1),
            Reaction::Focus(FocusTarget::Widget(id)) => {
                self.fields
                    .iter()
                    .position(|field| field.widget.id() == id)
                    .map_or(Reaction::Focus(FocusTarget::Widget(id)), |index| {
                        self.set_active_index(index)
                    })
            },
            Reaction::Changed => Reaction::Changed,
            Reaction::Ignored => Reaction::Ignored,
        }
    }

    fn current_value(&self) -> Option<Value> {
        Some(self.object_value())
    }
}

struct FormField {
    name:     String,
    widget:   Box<dyn Widget>,
    accepted: Option<Value>,
}

/// Draws a field, withholding the terminal cursor unless it has focus.
struct FieldWidget<'a> {
    widget: &'a dyn Widget,
    focused: bool,
}

impl screw::Widget for FieldWidget<'_> {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let cursor = out.cursor();
        self.widget.render(ctx, out);
        if !self.focused {
            match cursor {
                Some(position) => out.set_cursor(position),
                None => out.clear_cursor(),
            }
        }
    }

    fn tick_interest(&self) -> TickInterest {
        self.widget.tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.widget.vertical_size()
    }
}

#[cfg(test)]
mod tests {
    use screw::Surface;

    use super::*;
    use crate::{KeyEvent, widgets::TextInput};

    fn press(form: &mut Form, key: Key) -> Reaction {
        form.handle(Event::Key(KeyEvent::new(key)), &mut Context::new())
    }

    #[test]
    #[should_panic(expected = "form field name 'who' is already used")]
    fn a_repeated_field_name_is_refused() {
        let _ = Form::new("form")
            .with_field("who", TextInput::new("first"))
            .with_field("who", TextInput::new("second"));
    }

    fn required(id: &'static str) -> TextInput {
        TextInput::new(id).with_validator(|value| {
            if value.is_empty() {
                Err("required".to_owned())
            } else {
                Ok(())
            }
        })
    }

    #[test]
    fn tab_does_not_skip_a_validator_at_the_final_submit() {
        let mut form = Form::new("form")
            .with_field("name", required("name"))
            .with_field("nick", TextInput::new("nick"));
        press(&mut form, Key::Tab);

        let reaction = press(&mut form, Key::Enter);

        assert_eq!(reaction, Reaction::Focus(FocusTarget::Widget("name".into())));
        assert_eq!(form.active_index(), 0);

        press(&mut form, Key::Char('a'));
        press(&mut form, Key::Tab);
        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Submit(Value::Object(BTreeMap::from([
                ("name".to_owned(), Value::from("a")),
                ("nick".to_owned(), Value::from("")),
            ]))),
        );
    }

    #[test]
    fn a_refusing_last_field_stays_focused() {
        let mut form = Form::new("form").with_field("name", required("name"));
        assert_eq!(press(&mut form, Key::Enter), Reaction::Changed);
    }

    struct Hopper {
        value: &'static str,
    }

    impl screw::Widget for Hopper {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl Widget for Hopper {
        fn id(&self) -> WidgetId {
            WidgetId::from("hopper")
        }

        fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
            match event {
                Event::Key(key) if key.key == Key::Enter => Reaction::Submit(Value::from(self.value)),
                Event::Key(_) => {
                    self.value = "new";
                    Reaction::Focus(FocusTarget::Next)
                },
                _ => Reaction::Ignored,
            }
        }

        fn current_value(&self) -> Option<Value> {
            Some(Value::from(self.value))
        }
    }

    #[test]
    fn an_edit_answered_with_focus_invalidates_the_accepted_value() {
        let mut form = Form::new("form")
            .with_field("hop", Hopper { value: "old" })
            .with_field("nick", TextInput::new("nick"));
        press(&mut form, Key::Enter);
        press(&mut form, Key::Backtab);
        press(&mut form, Key::Char('x'));
        assert_eq!(form.active_index(), 1);

        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Submit(Value::Object(BTreeMap::from([
                ("hop".to_owned(), Value::from("new")),
                ("nick".to_owned(), Value::from("")),
            ]))),
        );
    }

    struct Answers;

    impl screw::Widget for Answers {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl Widget for Answers {
        fn id(&self) -> WidgetId {
            WidgetId::from("answers")
        }

        fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
            match event {
                Event::Key(key) if key.key == Key::Enter => Reaction::Submit(Value::from("answer")),
                _ => Reaction::Ignored,
            }
        }
    }

    struct Once {
        done: bool,
    }

    impl screw::Widget for Once {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl Widget for Once {
        fn id(&self) -> WidgetId {
            WidgetId::from("once")
        }

        fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
            match event {
                Event::Key(key) if key.key == Key::Enter && !self.done => {
                    self.done = true;
                    Reaction::Submit(Value::from("once"))
                },
                _ => Reaction::Ignored,
            }
        }
    }

    #[test]
    fn a_submit_once_widget_does_not_block_a_two_field_form() {
        let mut form = Form::new("form")
            .with_field("first", Once { done: false })
            .with_field("second", Once { done: false });
        press(&mut form, Key::Enter);
        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Submit(Value::Object(BTreeMap::from([
                ("first".to_owned(), Value::from("once")),
                ("second".to_owned(), Value::from("once")),
            ]))),
        );
    }

    struct Animated {
        done: bool,
    }

    impl screw::Widget for Animated {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl Widget for Animated {
        fn id(&self) -> WidgetId {
            WidgetId::from("animated")
        }

        fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
            match event {
                Event::Key(key) if key.key == Key::Enter && !self.done => {
                    self.done = true;
                    Reaction::Submit(Value::from("spun"))
                },
                Event::Tick => Reaction::Changed,
                _ => Reaction::Ignored,
            }
        }
    }

    #[test]
    fn a_tick_does_not_clear_an_accepted_value() {
        let mut form = Form::new("form")
            .with_field("spin", Animated { done: false })
            .with_field("nick", TextInput::new("nick"));
        press(&mut form, Key::Enter);
        press(&mut form, Key::Backtab);
        assert_eq!(form.handle(Event::Tick, &mut Context::new()), Reaction::Changed);
        press(&mut form, Key::Tab);

        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Submit(Value::Object(BTreeMap::from([
                ("spin".to_owned(), Value::from("spun")),
                ("nick".to_owned(), Value::from("")),
            ]))),
        );
    }

    struct Counter {
        count: i64,
        asked: usize,
    }

    impl screw::Widget for Counter {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl Widget for Counter {
        fn id(&self) -> WidgetId {
            WidgetId::from("counter")
        }

        fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
            match event {
                Event::Key(key) if key.key == Key::Enter => {
                    self.asked += 1;
                    Reaction::Submit(Value::from(self.count))
                },
                Event::Tick => {
                    self.count += 1;
                    Reaction::Changed
                },
                _ => Reaction::Ignored,
            }
        }

        fn current_value(&self) -> Option<Value> {
            Some(Value::from(self.count))
        }
    }

    #[test]
    fn a_tick_that_changes_the_value_invalidates_acceptance() {
        let mut form = Form::new("form")
            .with_field("count", Counter { count: 0, asked: 0 })
            .with_field("nick", TextInput::new("nick"));
        press(&mut form, Key::Enter);
        press(&mut form, Key::Backtab);
        form.handle(Event::Tick, &mut Context::new());
        press(&mut form, Key::Tab);

        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Submit(Value::Object(BTreeMap::from([
                ("count".to_owned(), Value::from(1)),
                ("nick".to_owned(), Value::from("")),
            ]))),
        );
    }

    #[test]
    fn a_field_edited_after_acceptance_is_validated_again() {
        let mut form = Form::new("form")
            .with_field("name", required("name"))
            .with_field("nick", TextInput::new("nick"));
        press(&mut form, Key::Char('a'));
        press(&mut form, Key::Enter);
        press(&mut form, Key::Backtab);
        press(&mut form, Key::Backspace);
        press(&mut form, Key::Tab);

        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Focus(FocusTarget::Widget("name".into())),
        );
    }

    #[test]
    fn a_submitted_value_survives_without_current_value() {
        let mut form = Form::new("form").with_field("custom", Answers);
        assert_eq!(
            press(&mut form, Key::Enter),
            Reaction::Submit(Value::Object(BTreeMap::from([(
                "custom".to_owned(),
                Value::from("answer")
            )]))),
        );
    }
}
