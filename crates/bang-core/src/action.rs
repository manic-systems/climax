// SPDX-License-Identifier: EUPL-1.2

use std::collections::BTreeMap;

use crate::{
    Context,
    Event,
    Key,
    KeyEvent,
    Modifiers,
    Reaction,
    Value,
    Widget,
    WidgetId,
};

/// A key bound to a named action, such as a review prompt's custom command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionBinding {
    key:  KeyEvent,
    name: String,
    help: String,
}

impl ActionBinding {
    /// Bind `key` to the action `name`. The help text starts as the name.
    #[must_use]
    pub fn new(key: KeyEvent, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            key,
            help: name.clone(),
            name,
        }
    }

    /// Bind the plain character `key` to the action `name`.
    #[must_use]
    pub fn char(key: char, name: impl Into<String>) -> Self {
        Self::new(KeyEvent::new(Key::Char(key)), name)
    }

    /// Bind Control and the character `key` to the action `name`.
    #[must_use]
    pub fn control_char(key: char, name: impl Into<String>) -> Self {
        Self::new(
            KeyEvent::with_modifiers(Key::Char(key), Modifiers::CONTROL),
            name,
        )
    }

    /// Set the text shown for this action in the key help.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// The key that triggers the action.
    #[must_use]
    pub const fn key_event(&self) -> &KeyEvent {
        &self.key
    }

    /// The name the action submits.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The text shown for this action in the key help.
    #[must_use]
    pub fn help(&self) -> &str {
        &self.help
    }

    fn matches(&self, key: &KeyEvent) -> bool {
        self.key == *key
    }
}

/// carries widget state + action executed
#[derive(Clone, Debug)]
pub struct ActionLayer<W> {
    widget:  W,
    actions: Vec<ActionBinding>,
}

impl<W> ActionLayer<W> {
    /// Wrap `widget` with no actions bound.
    #[must_use]
    pub const fn new(widget: W) -> Self {
        Self {
            widget,
            actions: Vec::new(),
        }
    }

    /// Replace the bound actions with `actions`.
    #[must_use]
    pub fn with_actions(mut self, actions: impl IntoIterator<Item = ActionBinding>) -> Self {
        self.actions = actions.into_iter().collect();
        self
    }

    /// Bind one more action.
    pub fn push_action(&mut self, action: ActionBinding) {
        self.actions.push(action);
    }

    /// The bound actions in the order they were added.
    #[must_use]
    pub fn actions(&self) -> &[ActionBinding] {
        &self.actions
    }

    /// Give back the wrapped widget.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.widget
    }
}

impl<W> screw::Widget for ActionLayer<W>
where
    W: Widget,
{
    fn render(&self, ctx: &screw::RenderCtx, out: &mut screw::Surface) {
        self.widget.render(ctx, out);
    }

    fn tick_interest(&self) -> screw::TickInterest {
        self.widget.tick_interest()
    }

    fn vertical_size(&self) -> screw::VerticalSize {
        self.widget.vertical_size()
    }
}

impl<W> Widget for ActionLayer<W>
where
    W: Widget,
{
    fn id(&self) -> WidgetId {
        self.widget.id()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        if let Event::Key(key) = &event
            && let Some(action) = self.actions.iter().find(|action| action.matches(key))
        {
            return Reaction::Action(action_output(
                action.name(),
                self.widget.current_value().unwrap_or(Value::Null),
            ));
        }

        self.widget.handle(event, cx)
    }

    fn current_value(&self) -> Option<Value> {
        self.widget.current_value()
    }
}

fn action_output(action: &str, value: Value) -> Value {
    Value::Object(BTreeMap::from([
        ("action".to_owned(), Value::from(action)),
        ("value".to_owned(), value),
    ]))
}

#[cfg(test)]
mod tests {
    use crate::{
        ActionBinding,
        ActionLayer,
        Event,
        Key,
        KeyEvent,
        Modifiers,
        Session,
        SessionReaction,
        SessionStatus,
        Value,
        widgets::{
            SelectItem,
            TextInput,
        },
    };

    #[test]
    fn action_layer_submits_current_value_for_bound_control_key() {
        let mut session = Session::new(
            ActionLayer::new(TextInput::new("name").with_value("Ada"))
                .with_actions([ActionBinding::control_char('s', "save")]),
        );

        assert_eq!(session.handle(Event::char('!')), SessionReaction::Changed);
        assert!(matches!(
            session.handle(Event::Key(KeyEvent::with_modifiers(
                Key::Char('s'),
                Modifiers::CONTROL
            ))),
            SessionReaction::Submit(_)
        ));

        let SessionStatus::Submitted(Value::Object(value)) = session.status() else {
            panic!("action layer should submit an action object");
        };
        assert_eq!(value.get("action"), Some(&Value::from("save")));
        assert_eq!(value.get("value"), Some(&Value::from("Ada!")));
    }

    #[test]
    fn action_layer_inside_a_form_leaves_the_form() {
        let mut session = Session::new(
            crate::widgets::Form::new("form")
                .with_field(
                    "name",
                    ActionLayer::new(TextInput::new("name").with_value("Ada"))
                        .with_actions([ActionBinding::control_char('s', "save")]),
                )
                .with_field("note", TextInput::new("note")),
        );

        session.handle(Event::Key(KeyEvent::with_modifiers(
            Key::Char('s'),
            Modifiers::CONTROL,
        )));
        let SessionStatus::Submitted(Value::Object(value)) = session.status() else {
            panic!("a bound action should end the form");
        };
        assert_eq!(value.get("action"), Some(&Value::from("save")));
    }

    #[test]
    fn action_layer_passes_unbound_events_to_inner_widget() {
        let mut session = Session::new(
            ActionLayer::new(crate::widgets::Select::new("choice", [
                SelectItem::from("alpha"),
                SelectItem::from("bravo"),
            ]))
            .with_actions([ActionBinding::control_char('s', "save")]),
        );

        assert_eq!(session.handle(Event::key(Key::Down)), SessionReaction::Changed);
        assert!(matches!(
            session.handle(Event::key(Key::Enter)),
            SessionReaction::Submit(_)
        ));

        assert_eq!(
            session.status(),
            &SessionStatus::Submitted(Value::from("bravo"))
        );
    }
}
