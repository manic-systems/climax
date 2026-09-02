//! Advanced widget and session APIs.
//!
//! Most applications should use the typed prompt builders at the crate root.

pub use bang_core::{
    ActionBinding, ActionLayer, Context, Event, FocusTarget, Key, KeyEvent, Modifiers, Reaction,
    Session, SessionStatus, Value, Widget, widgets,
};

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
pub fn replay_events(
    widget: impl Widget + 'static,
    events: impl IntoIterator<Item = Event>,
) -> Result<Value> {
    let mut session = Session::new(widget);
    for event in events {
        match session.handle(event) {
            Reaction::Submit(value) => return Ok(value),
            Reaction::Cancel => return Err(Error::cancelled()),
            Reaction::Ignored | Reaction::Changed | Reaction::Focus(_) => {},
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
