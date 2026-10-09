// SPDX-License-Identifier: EUPL-1.2

//! Advanced widget and session APIs.
//!
//! Most applications should use the typed prompt builders at the crate root.
//!
//! A [`Widget`] adds input handling to a `screw` widget. To write one,
//! implement `screw::Widget` to draw into a `Surface` and then [`Widget`] to
//! handle events. To host a built-in widget in your own `screw` layout, render
//! it into a `Surface` like any other `screw` widget. `screw` is re-exported
//! whole as [`crate::screw`], so name `Surface` and `RenderCtx` through it and
//! stay on the version bang was built against. The terminal toolkit lives in
//! [`crate::terminal`].

pub use bang_core::{
    ActionBinding, ActionLayer, Context as WidgetContext, Date, Event, FocusTarget, Key, KeyEvent,
    Modifiers, Number, Reaction, Session, SessionReaction, SessionStatus, Value, Widget, widgets,
};

pub use bang_core::WidgetId;

pub use crate::live::LiveSessionError;

pub use crate::interaction::InteractionWidget;
use crate::{Error, Interaction, Result};

/// Run a custom widget in the live terminal session.
pub fn interact_widget(
    widget: impl Widget + 'static,
    actions: impl IntoIterator<Item = ActionBinding>,
) -> Result<Value> {
    Interaction::live().interact(widget, actions)
}

/// Build a deterministic interaction driver from one event sequence per prompt.
#[must_use]
pub fn scripted_interaction(
    scripts: impl IntoIterator<Item = impl IntoIterator<Item = Event>>,
) -> Interaction {
    crate::interaction::scripted(scripts)
}

/// Build an interaction driver around a custom advanced session runner.
#[must_use]
pub fn interaction_from_runner(
    runner: impl Fn(InteractionWidget) -> Result<Value> + 'static,
) -> Interaction {
    Interaction::from_runner(move |widget| runner(InteractionWidget::new(widget)))
}

/// Drive a custom widget with already-decoded events.
///
/// Ctrl-C and Ctrl-D are offered to the widget first, matching the live runner.
/// A widget that returns anything but [`Reaction::Ignored`] has claimed the key
/// and replay continues; an ignored Ctrl-C cancels and an ignored Ctrl-D ends
/// input.
pub fn replay_events(
    widget: impl Widget + 'static,
    events: impl IntoIterator<Item = Event>,
) -> Result<Value> {
    let mut session = Session::new(widget);
    for event in events {
        let unhandled = unhandled_error(&event);
        match session.handle(event) {
            SessionReaction::Submit(value) => return Ok(value),
            SessionReaction::Cancel => return Err(Error::cancelled()),
            SessionReaction::Ignored => {
                if let Some(error) = unhandled {
                    return Err(error);
                }
            },
            SessionReaction::Changed | SessionReaction::Focus(_) => {},
        }
        if !matches!(session.status(), SessionStatus::Running) {
            break;
        }
    }

    match session.status() {
        SessionStatus::Submitted(value) => Ok(value.clone()),
        SessionStatus::Cancelled => Err(Error::cancelled()),
        SessionStatus::Running => Err(Error::input_ended()),
    }
}

/// The error a reserved key falls back to when no widget claims it.
fn unhandled_error(event: &Event) -> Option<Error> {
    if is_control_char(event, 'c') {
        return Some(Error::cancelled());
    }
    if is_control_char(event, 'd') {
        return Some(Error::input_ended());
    }
    None
}

fn is_control_char(event: &Event, value: char) -> bool {
    matches!(
        event,
        Event::Key(key)
            if key.key == Key::Char(value) && key.modifiers.contains(Modifiers::CONTROL)
    )
}

#[cfg(test)]
mod tests {
    use bang_core::{ActionBinding, ActionLayer, widgets::TextInput};

    use super::*;

    fn control(value: char) -> Event {
        Event::Key(KeyEvent::with_modifiers(
            Key::Char(value),
            Modifiers::CONTROL,
        ))
    }

    #[test]
    fn unclaimed_reserved_keys_cancel_and_end_input() {
        assert_eq!(
            replay_events(TextInput::new("value"), [control('c')])
                .unwrap_err()
                .kind(),
            crate::ErrorKind::Cancelled,
        );
        assert_eq!(
            replay_events(TextInput::new("value"), [control('d')])
                .unwrap_err()
                .kind(),
            crate::ErrorKind::InputEnded,
        );
    }

    #[test]
    fn an_action_binding_can_claim_a_reserved_key() {
        let widget = ActionLayer::new(TextInput::new("value"))
            .with_actions([ActionBinding::control_char('c', "copy")]);

        let value = replay_events(widget, [control('c')]).unwrap();

        let bang_core::Value::Object(fields) = value else {
            panic!("an action binding submits an object, got {value:?}");
        };
        assert_eq!(fields["action"], bang_core::Value::from("copy"));
    }
}
