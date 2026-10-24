// SPDX-License-Identifier: EUPL-1.2

use alloc::collections::BTreeMap;

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

/// a key bound to a named action, such as a review prompt's custom command
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionBinding {
    key:  KeyEvent,
    name: String,
    help: String,
}

impl ActionBinding {
    /// binds `key` to the action `name`, help starts as the name
    #[must_use]
    pub fn new(key: KeyEvent, name: impl Into<String>) -> Self {
        let owned = name.into();
        Self {
            key,
            help: owned.clone(),
            name: owned,
        }
    }

    /// binds the plain character `key` to the action `name`
    #[must_use]
    pub fn char(key: char, name: impl Into<String>) -> Self {
        Self::new(KeyEvent::new(Key::Char(key)), name)
    }

    /// binds Control and the character `key` to the action `name`
    #[must_use]
    pub fn control_char(key: char, name: impl Into<String>) -> Self {
        Self::new(
            KeyEvent::with_modifiers(Key::Char(key), Modifiers::CONTROL),
            name,
        )
    }

    /// sets the text shown for this action in the key help
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }

    /// the key that triggers the action
    #[must_use]
    pub const fn key_event(&self) -> &KeyEvent {
        &self.key
    }

    /// the name the action submits
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// the text shown for this action in the key help
    #[must_use]
    pub fn help(&self) -> &str {
        &self.help
    }

    fn help_text(&self) -> String {
        format!("{} {}", key_label(&self.key), self.help)
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
    /// wraps `widget` with no actions bound
    #[must_use]
    pub const fn new(widget: W) -> Self {
        Self {
            widget,
            actions: Vec::new(),
        }
    }

    /// replaces the bound actions with `actions`
    #[must_use]
    pub fn with_actions(mut self, actions: impl IntoIterator<Item = ActionBinding>) -> Self {
        self.actions = actions.into_iter().collect();
        self
    }

    /// binds one more action
    pub fn push_action(&mut self, action: ActionBinding) {
        self.actions.push(action);
    }

    /// the bound actions in the order they were added
    #[must_use]
    pub fn actions(&self) -> &[ActionBinding] {
        &self.actions
    }

    /// gives back the wrapped widget
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
        if self.actions.is_empty() {
            return;
        }
        let help = self
            .actions
            .iter()
            .map(ActionBinding::help_text)
            .collect::<Vec<_>>()
            .join(" | ");
        let style = ctx.theme().style(screw::Role::Dim);
        if self.widget.has_key_help() {
            out.write(format!(" | {help}"), style);
        } else {
            out.newline();
            out.write(help, style);
        }
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

    fn has_key_help(&self) -> bool {
        self.widget.has_key_help() || !self.actions.is_empty()
    }

    fn offers_nothing(&self) -> bool {
        self.widget.offers_nothing()
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

fn key_label(key: &KeyEvent) -> String {
    let mut parts = Vec::new();
    for (modifier, name) in [
        (Modifiers::CONTROL, "ctrl"),
        (Modifiers::ALT, "alt"),
        (Modifiers::SHIFT, "shift"),
        (Modifiers::SUPER, "super"),
    ] {
        if key.modifiers.contains(modifier) {
            parts.push(name.to_owned());
        }
    }
    parts.push(key_name(&key.key));
    parts.join("-")
}

fn key_name(key: &Key) -> String {
    match key {
        Key::Char(value) => value.to_string(),
        Key::Function(number) => format!("f{number}"),
        Key::Enter => "enter".to_owned(),
        Key::Esc => "esc".to_owned(),
        Key::Tab => "tab".to_owned(),
        Key::Backtab => "backtab".to_owned(),
        Key::Backspace => "backspace".to_owned(),
        Key::Delete => "delete".to_owned(),
        Key::Up => "up".to_owned(),
        Key::Down => "down".to_owned(),
        Key::Left => "left".to_owned(),
        Key::Right => "right".to_owned(),
        Key::Home => "home".to_owned(),
        Key::End => "end".to_owned(),
        Key::PageUp => "pgup".to_owned(),
        Key::PageDown => "pgdn".to_owned(),
        Key::Insert => "insert".to_owned(),
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
            Form,
            Select,
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

    fn help_line(widget: &impl screw::Widget) -> String {
        let mut surface = screw::Surface::new();
        widget.render(
            &screw::RenderCtx::new().with_constraints(Some(80), None),
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
    fn bound_actions_extend_the_key_help_line() {
        let layer = ActionLayer::new(Select::new("choice", ["alpha"])).with_actions([
            ActionBinding::control_char('s', "save"),
            ActionBinding::char('g', "go").with_help("regenerate"),
        ]);
        assert_eq!(
            help_line(&layer),
            "up/down move | enter submit | esc cancel | ctrl-s save | g regenerate"
        );
    }

    #[test]
    fn a_widget_without_key_help_gets_a_help_line_of_its_own() {
        let layer = ActionLayer::new(TextInput::new("name"))
            .with_actions([ActionBinding::control_char('s', "save")]);
        assert_eq!(help_line(&layer), "ctrl-s save");
        assert_eq!(help_line(&ActionLayer::new(TextInput::new("name"))), "");
    }

    #[test]
    fn action_layer_inside_a_form_leaves_the_form() {
        let mut session = Session::new(
            Form::new("form")
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
            ActionLayer::new(Select::new("choice", [
                SelectItem::from("alpha"),
                SelectItem::from("bravo"),
            ]))
            .with_actions([ActionBinding::control_char('s', "save")]),
        );

        assert_eq!(
            session.handle(Event::key(Key::Down)),
            SessionReaction::Changed
        );
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
