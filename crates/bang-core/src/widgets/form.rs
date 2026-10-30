// SPDX-License-Identifier: EUPL-1.2

use alloc::collections::BTreeMap;
use core::{
    cell::Cell,
    fmt,
};

use screw::{
    LayoutMode,
    LocalWidgetRef,
    RenderCtx,
    Role,
    Span,
    Spans,
    Surface,
    Text,
    TickInterest,
    VerticalSize,
    VerticalViewport,
    combine_tick_interest,
    local_widget,
};

use super::navigation::chorded;
use crate::{
    Context,
    Event,
    FocusTarget,
    Key,
    Reaction,
    Value,
    Widget,
    WidgetId,
};

/// several named widgets shown together, one active at a time, submits an
/// object of values by field name, a field is asked again only after it changes
///
/// given a height, the form scrolls to keep the active field in view and pins
/// its key help at the bottom
pub struct Form {
    id:     WidgetId,
    fields: Vec<FormField>,
    active: usize,
    top:    Cell<usize>,
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Form")
            .field("id", &self.id)
            .field(
                "fields",
                &self
                    .fields
                    .iter()
                    .map(|field| field.name.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl Form {
    /// an empty form
    #[must_use]
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id:     id.into(),
            fields: Vec::new(),
            active: 0,
            top:    Cell::new(0),
        }
    }

    /// adds a field called `name` that edits through `widget`
    #[must_use]
    pub fn with_field(mut self, name: impl Into<String>, widget: impl Widget + 'static) -> Self {
        self.push_field(name, widget);
        self
    }

    /// adds a field called `name` that edits through `widget`
    ///
    /// # Panics
    ///
    /// panics when a field called `name` already exists, when `widget` has the
    /// id of an existing field, or when `widget` offers nothing to pick
    pub fn push_field(&mut self, name: impl Into<String>, widget: impl Widget + 'static) {
        let owned = name.into();
        assert!(
            self.fields.iter().all(|field| field.name != owned),
            "form field name '{owned}' is already used"
        );
        assert!(
            !widget.offers_nothing(),
            "form field '{owned}' has nothing to pick"
        );
        let widget_id = widget.id();
        assert!(
            self.fields
                .iter()
                .all(|field| field.widget.id() != widget_id),
            "form widget id '{}' is already used",
            widget_id.as_str()
        );
        self.fields.push(FormField {
            name:     owned,
            widget:   Box::new(widget),
            accepted: None,
        });
        self.active = self.active.min(self.fields.len().saturating_sub(1));
    }

    /// how many fields the form has
    #[must_use]
    pub const fn len(&self) -> usize {
        self.fields.len()
    }

    /// whether the form has no fields
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// the index of the field that receives input
    #[must_use]
    pub const fn active_index(&self) -> usize {
        self.active
    }

    /// the id of the active field's widget, or `None` for an empty form
    #[must_use]
    pub fn active_widget_id(&self) -> Option<WidgetId> {
        self.fields.get(self.active).map(|field| field.widget.id())
    }

    /// makes the field at `active` receive input, clamped to the last field
    pub fn set_active_index(&mut self, active: usize) -> Reaction {
        if self.fields.is_empty() {
            self.active = 0;
            return Reaction::Ignored;
        }

        let clamped = active.min(self.fields.len() - 1);
        if clamped == self.active {
            return Reaction::Ignored;
        }
        self.active = clamped;
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

    /// asks every unaccepted field to accept by sending it Enter, focus lands
    /// on the first that refuses
    fn submit_all(&mut self) -> Reaction {
        let mut values = BTreeMap::new();
        for index in 0..self.fields.len() {
            let value = if let Some(value) = self.fields[index].accepted.clone() {
                value
            } else {
                let mut cx = Context::new();
                match self.fields[index]
                    .widget
                    .handle(Event::key(Key::Enter), &mut cx)
                {
                    Reaction::Submit(value) => {
                        self.fields[index].accepted = Some(value.clone());
                        value
                    },
                    Reaction::Action(value) => return Reaction::Action(value),
                    Reaction::Cancel => return Reaction::Cancel,
                    Reaction::Ignored | Reaction::Changed | Reaction::Focus(_) => {
                        return self.refuse(index);
                    },
                }
            };
            values.insert(self.fields[index].name.clone(), value);
        }
        Reaction::Submit(Value::Object(values))
    }

    fn deliver(&mut self, index: usize, event: Event, cx: &mut Context) -> Reaction {
        let field = &mut self.fields[index];
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
        reaction
    }

    fn broadcast(&mut self, event: &Event, cx: &mut Context) -> Reaction {
        let ticks = matches!(event, Event::Tick);
        let mut result = Reaction::Ignored;
        for index in 0..self.fields.len() {
            if ticks && self.fields[index].widget.tick_interest() == TickInterest::Never {
                continue;
            }
            match self.deliver(index, event.clone(), cx) {
                reaction @ (Reaction::Action(_) | Reaction::Cancel) => return reaction,
                Reaction::Ignored => {},
                Reaction::Changed | Reaction::Submit(_) | Reaction::Focus(_) => {
                    result = Reaction::Changed;
                },
            }
        }
        result
    }

    fn unhandled(&mut self, event: &Event) -> Reaction {
        match event {
            Event::Key(key) => {
                match key.key {
                    Key::Tab | Key::Backtab | Key::Esc | Key::Enter if chorded(key) => {
                        Reaction::Ignored
                    },
                    Key::Tab => self.move_focus(1),
                    Key::Backtab => self.move_focus(-1),
                    Key::Esc => Reaction::Cancel,
                    Key::Enter if self.fields.is_empty() => Reaction::Submit(self.object_value()),
                    Key::Enter
                    | Key::Char(_)
                    | Key::Backspace
                    | Key::Delete
                    | Key::Up
                    | Key::Down
                    | Key::Left
                    | Key::Right
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown
                    | Key::Insert
                    | Key::Function(_) => Reaction::Ignored,
                }
            },
            Event::Paste(_)
            | Event::UnknownEscape(_)
            | Event::Resize { .. }
            | Event::Tick
            | Event::Row(_) => Reaction::Ignored,
        }
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
        let Some(active) = self.fields.get(self.active) else {
            return;
        };
        let remaining = ctx
            .available_rows()
            .map(|rows| rows.saturating_sub(out.height().saturating_sub(1)));
        let help = help_line(active);
        let help_rows = wrapped_rows(&help, ctx);
        let smallest_body = if active.widget.vertical_size() == VerticalSize::Flexible {
            2
        } else {
            1
        };
        let pinned = remaining.is_none_or(|rows| rows >= help_rows + 1 + smallest_body);
        let budget = remaining.map(|rows| rows.saturating_sub(if pinned { help_rows } else { 0 }));

        let blocks: Vec<LocalWidgetRef<'_>> = self
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let focused = index == self.active;
                local_widget(FieldBlock {
                    name: &field.name,
                    widget: &*field.widget,
                    focused,
                    labelled: !focused || budget.is_none_or(|rows| rows > smallest_body),
                    rows: if focused { budget } else { None },
                })
            })
            .collect();
        let mut viewport = VerticalViewport::new(blocks)
            .requested_start(self.top.get())
            .anchor(Some(self.active));
        if pinned {
            viewport = viewport.trailing(local_widget(Text::new(help).role(Role::Dim)));
        }
        let report = viewport.report_handle();
        viewport.render(ctx, out);
        if remaining.is_some() {
            self.top.set(report.report().visible.start);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(self.fields.iter().map(|field| field.widget.tick_interest()))
    }

    fn vertical_size(&self) -> VerticalSize {
        if self.fields.is_empty() {
            VerticalSize::Content
        } else {
            VerticalSize::Flexible
        }
    }
}

fn help_line(active: &FormField) -> String {
    let mut keys = Vec::new();
    if !active.widget.takes_tab() {
        keys.push("tab next");
    }
    keys.push("shift-tab previous");
    if !active.widget.has_key_help() {
        keys.extend(["enter accept", "esc cancel"]);
    }
    keys.join(" | ")
}

fn wrapped_rows(help: &str, ctx: &RenderCtx) -> usize {
    if ctx.layout_mode() == LayoutMode::Wrap
        && let Some(columns) = ctx.available_columns()
    {
        screw::width(help).div_ceil(columns.max(1))
    } else {
        1
    }
}

impl Widget for Form {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn has_key_help(&self) -> bool {
        !self.fields.is_empty()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        if matches!(event, Event::Resize { .. } | Event::Tick) {
            return self.broadcast(&event, cx);
        }

        let reaction = if self.fields.is_empty() {
            Reaction::Ignored
        } else {
            self.deliver(self.active, event.clone(), cx)
        };
        match reaction {
            Reaction::Action(value) => Reaction::Action(value),
            Reaction::Submit(value) => self.submit_or_advance(value),
            Reaction::Cancel => Reaction::Cancel,
            Reaction::Focus(FocusTarget::Next) => self.move_focus(1),
            Reaction::Focus(FocusTarget::Previous) => self.move_focus(-1),
            Reaction::Focus(target) => Reaction::Focus(target),
            Reaction::Changed => Reaction::Changed,
            Reaction::Ignored => self.unhandled(&event),
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

struct FieldBlock<'a> {
    name:     &'a str,
    widget:   &'a dyn Widget,
    focused:  bool,
    labelled: bool,
    rows:     Option<usize>,
}

impl screw::Widget for FieldBlock<'_> {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        if self.labelled {
            Spans::new([
                Span::new(if self.focused { "> " } else { "  " }).role(Role::Dim),
                Span::new(self.name).role(if self.focused {
                    Role::Selected
                } else {
                    Role::Dim
                }),
            ])
            .render(ctx, out);
            out.newline();
        }
        let cursor = out.cursor();
        self.widget.render(
            &ctx.with_constraints(ctx.available_columns(), self.rows),
            out,
        );
        if !self.focused {
            match cursor {
                Some(position) => out.set_cursor(position),
                None => out.clear_cursor(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use screw::Surface;

    use super::*;
    use crate::{
        ActionLayer,
        KeyEvent,
        widgets::{
            SearchSelect,
            Select,
            TextInput,
        },
    };

    fn press(form: &mut Form, key: Key) -> Reaction {
        form.handle(Event::Key(KeyEvent::new(key)), &mut Context::new())
    }

    #[test]
    #[should_panic(expected = "form field name 'who' is already used")]
    fn a_repeated_field_name_is_refused() {
        drop(
            Form::new("form")
                .with_field("who", TextInput::new("first"))
                .with_field("who", TextInput::new("second")),
        );
    }

    #[test]
    #[should_panic(expected = "form widget id 'same' is already used")]
    fn a_repeated_widget_id_is_refused() {
        drop(
            Form::new("form")
                .with_field("first", TextInput::new("same"))
                .with_field("second", TextInput::new("same")),
        );
    }

    #[test]
    #[should_panic(expected = "form field 'pick' has nothing to pick")]
    fn an_empty_select_is_refused() {
        drop(Form::new("form").with_field("pick", Select::new("pick", Vec::<String>::new())));
    }

    #[test]
    #[should_panic(expected = "form field 'pick' has nothing to pick")]
    fn an_empty_search_select_is_refused() {
        drop(Form::new("form").with_field("pick", SearchSelect::new("pick", Vec::<String>::new())));
    }

    #[test]
    #[should_panic(expected = "form field 'pick' has nothing to pick")]
    fn an_empty_select_under_an_action_layer_is_refused() {
        drop(Form::new("form").with_field(
            "pick",
            ActionLayer::new(Select::new("pick", Vec::<String>::new())),
        ));
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

        assert_eq!(
            reaction,
            Reaction::Focus(FocusTarget::Widget("name".into()))
        );
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
                Event::Key(key) if key.key == Key::Enter => {
                    Reaction::Submit(Value::from(self.value))
                },
                Event::Key(_) => {
                    self.value = "new";
                    Reaction::Focus(FocusTarget::Next)
                },
                Event::Paste(_)
                | Event::UnknownEscape(_)
                | Event::Resize { .. }
                | Event::Tick
                | Event::Row(_) => Reaction::Ignored,
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
                Event::Key(_)
                | Event::Paste(_)
                | Event::UnknownEscape(_)
                | Event::Resize { .. }
                | Event::Tick
                | Event::Row(_) => Reaction::Ignored,
            }
        }
    }

    struct Once {
        id:   &'static str,
        done: bool,
    }

    impl screw::Widget for Once {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}
    }

    impl Widget for Once {
        fn id(&self) -> WidgetId {
            WidgetId::from(self.id)
        }

        fn handle(&mut self, event: Event, _cx: &mut Context) -> Reaction {
            match event {
                Event::Key(key) if key.key == Key::Enter && !self.done => {
                    self.done = true;
                    Reaction::Submit(Value::from("once"))
                },
                Event::Key(_)
                | Event::Paste(_)
                | Event::UnknownEscape(_)
                | Event::Resize { .. }
                | Event::Tick
                | Event::Row(_) => Reaction::Ignored,
            }
        }
    }

    #[test]
    fn a_submit_once_widget_does_not_block_a_two_field_form() {
        let mut form = Form::new("form")
            .with_field("first", Once {
                id:   "first",
                done: false,
            })
            .with_field("second", Once {
                id:   "second",
                done: false,
            });
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

        fn tick_interest(&self) -> TickInterest {
            TickInterest::EveryFrame
        }
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
                Event::Key(_)
                | Event::Paste(_)
                | Event::UnknownEscape(_)
                | Event::Resize { .. }
                | Event::Row(_) => Reaction::Ignored,
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
        assert_eq!(
            form.handle(Event::Tick, &mut Context::new()),
            Reaction::Changed
        );
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
        id:    &'static str,
        count: i64,
        asked: usize,
    }

    impl screw::Widget for Counter {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {}

        fn tick_interest(&self) -> TickInterest {
            TickInterest::EveryFrame
        }
    }

    impl Widget for Counter {
        fn id(&self) -> WidgetId {
            WidgetId::from(self.id)
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
                Event::Key(_)
                | Event::Paste(_)
                | Event::UnknownEscape(_)
                | Event::Resize { .. }
                | Event::Row(_) => Reaction::Ignored,
            }
        }

        fn current_value(&self) -> Option<Value> {
            Some(Value::from(self.count))
        }
    }

    #[test]
    fn a_tick_that_changes_the_value_invalidates_acceptance() {
        let mut form = Form::new("form")
            .with_field("count", Counter {
                id:    "counter",
                count: 0,
                asked: 0,
            })
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
    fn a_tick_reaches_every_field_that_wants_one() {
        let mut form = Form::new("form")
            .with_field("a", Counter {
                id:    "a",
                count: 0,
                asked: 0,
            })
            .with_field("b", Counter {
                id:    "b",
                count: 0,
                asked: 0,
            });
        assert_eq!(
            form.handle(Event::Tick, &mut Context::new()),
            Reaction::Changed
        );
        assert_eq!(
            form.current_value(),
            Some(Value::Object(BTreeMap::from([
                ("a".to_owned(), Value::from(1)),
                ("b".to_owned(), Value::from(1)),
            ]))),
        );
    }

    #[test]
    fn a_widget_that_takes_esc_keeps_it_inside_a_form() {
        use crate::{
            ActionBinding,
            ActionLayer,
            widgets::ReviewList,
        };

        let mut review = Form::new("form").with_field(
            "review",
            ReviewList::new("review", ["alpha"])
                .with_leave_output(true)
                .with_exit_output(true),
        );
        assert!(matches!(press(&mut review, Key::Esc), Reaction::Action(_)));

        let mut bound = Form::new("form").with_field(
            "bound",
            ActionLayer::new(TextInput::new("bound"))
                .with_actions([ActionBinding::new(KeyEvent::new(Key::Esc), "stop")]),
        );
        assert!(matches!(press(&mut bound, Key::Esc), Reaction::Action(_)));

        let mut plain = Form::new("form").with_field("plain", TextInput::new("plain"));
        assert_eq!(press(&mut plain, Key::Esc), Reaction::Cancel);
    }

    #[test]
    fn a_widget_that_takes_tab_keeps_it_inside_a_form() {
        use crate::widgets::MultiSelect;

        let mut pick = Form::new("form")
            .with_field("pick", MultiSelect::new("pick", ["alpha"]))
            .with_field("note", TextInput::new("note"));
        assert_eq!(press(&mut pick, Key::Tab), Reaction::Changed);
        assert_eq!(pick.active_index(), 0);
        assert_eq!(
            pick.current_value(),
            Some(Value::Object(BTreeMap::from([
                ("note".to_owned(), Value::from("")),
                ("pick".to_owned(), Value::List(vec![Value::from("alpha")])),
            ]))),
        );

        let mut plain = Form::new("form")
            .with_field("a", TextInput::new("a"))
            .with_field("b", TextInput::new("b"));
        press(&mut plain, Key::Tab);
        assert_eq!(plain.active_index(), 1);
    }

    fn help_under(widget: impl Widget + 'static) -> String {
        let form = Form::new("form").with_field("field", widget);
        let mut surface = Surface::new();
        screw::Widget::render(
            &form,
            &RenderCtx::new().with_constraints(Some(80), None),
            &mut surface,
        );
        surface
            .plain_text()
            .lines()
            .last()
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn the_help_line_fits_the_active_field() {
        use crate::{
            ActionBinding,
            widgets::{
                MultiSelect,
                ReviewList,
            },
        };

        assert_eq!(
            help_under(TextInput::new("text")),
            "tab next | shift-tab previous | enter accept | esc cancel"
        );
        assert_eq!(
            help_under(Select::new("pick", ["alpha"])),
            "tab next | shift-tab previous"
        );
        assert_eq!(
            help_under(MultiSelect::new("pick", ["alpha"])),
            "shift-tab previous"
        );
        assert_eq!(
            help_under(ReviewList::new("pick", ["alpha"])),
            "shift-tab previous"
        );
        assert_eq!(
            help_under(ActionLayer::new(MultiSelect::new("pick", ["alpha"]))),
            "shift-tab previous"
        );
        assert_eq!(
            help_under(
                ActionLayer::new(TextInput::new("text"))
                    .with_actions([ActionBinding::new(KeyEvent::new(Key::Function(1)), "help")])
            ),
            "tab next | shift-tab previous | enter accept | esc cancel"
        );
    }

    #[test]
    fn a_nested_form_cannot_move_the_outer_focus() {
        let inner = Form::new("inner")
            .with_field("one", TextInput::new("one"))
            .with_field("two", TextInput::new("shared"));
        let mut outer = Form::new("outer")
            .with_field("first", TextInput::new("shared"))
            .with_field("inner", inner);
        outer.set_active_index(1);

        assert_eq!(
            press(&mut outer, Key::Tab),
            Reaction::Focus(FocusTarget::Widget("shared".into()))
        );
        assert_eq!(outer.active_index(), 1);
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
