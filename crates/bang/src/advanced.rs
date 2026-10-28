// SPDX-License-Identifier: EUPL-1.2

//! advanced: widgets, sessions and replay, for what the typed prompt builders
//! do not cover
//!
//! a [`Widget`] adds input handling to a `screw` widget, so implement
//! `screw::Widget` to draw into a `Surface` and then [`Widget`] to handle
//! events. a built-in widget renders into a `Surface` like any other `screw`
//! widget. the terminal toolkit is in [`crate::terminal`]

pub use bang_core::{
    ActionBinding,
    ActionLayer,
    Context as WidgetContext,
    Date,
    Event,
    FocusTarget,
    Key,
    KeyEvent,
    Modifiers,
    Number,
    OutputFormat,
    Reaction,
    Session,
    SessionReaction,
    SessionStatus,
    Value,
    Widget,
    WidgetId,
    format_output,
    widgets,
};

use crate::{
    Error,
    Interaction,
    Result,
    interaction::{
        scripted,
        scripted_checked,
    },
    session::{
        RunOutcome,
        unhandled_outcome,
    },
};
pub use crate::{
    input::Headed,
    interaction::{
        InteractionWidget,
        ScriptCheck,
    },
    live::LiveSessionError,
};

/// run a custom widget in the live terminal session
///
/// # Errors
///
/// fails when the live session cannot start or ends without a value
pub fn interact_widget(
    widget: impl Widget + 'static,
    actions: impl IntoIterator<Item = ActionBinding>,
) -> Result<Value> {
    Interaction::live().interact(widget, actions)
}

/// build a deterministic interaction driver from one event sequence per prompt
///
/// panics when the last clone of the driver is dropped with a script never
/// started or events left unread, unless a guard or the prompt itself rejected
/// the interaction first
#[must_use]
pub fn scripted_interaction(
    scripts: impl IntoIterator<Item = impl IntoIterator<Item = Event>>,
) -> Interaction {
    scripted(scripts)
}

/// build a scripted driver like [`scripted_interaction`], with a
/// [`ScriptCheck`] to ask what it left unread
///
/// dropping the driver never panics, the caller checks instead. clones of the
/// driver share the script and the check
#[must_use]
pub fn checked_scripted_interaction(
    scripts: impl IntoIterator<Item = impl IntoIterator<Item = Event>>,
) -> (Interaction, ScriptCheck) {
    scripted_checked(scripts)
}

/// build an interaction driver around a custom advanced session runner
#[must_use]
pub fn interaction_from_runner(
    runner: impl Fn(InteractionWidget) -> Result<Value> + 'static,
) -> Interaction {
    Interaction::from_runner(move |widget| runner(InteractionWidget::new(widget)))
}

/// drive a custom widget with already-decoded events
///
/// ctrl-c and ctrl-d are offered to the widget first, as in the live runner,
/// and an ignored ctrl-c cancels while an ignored ctrl-d ends input. an
/// `Event::Row` the widget ignores fails, since the row is past the end or the
/// widget takes no rows
///
/// # Errors
///
/// fails with `ErrorKind::Cancelled` when the widget or an unclaimed ctrl-c
/// cancels, with `ErrorKind::InputEnded` when the events run out before the
/// widget submits, and with `ErrorKind::InvalidConfiguration` for an ignored
/// row
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "the session enums are non-exhaustive"
)]
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
            _ => {},
        }
        if !matches!(session.status(), SessionStatus::Running) {
            break;
        }
    }

    match session.status() {
        SessionStatus::Submitted(value) => Ok(value.clone()),
        SessionStatus::Cancelled => Err(Error::cancelled()),
        _ => Err(Error::input_ended()),
    }
}

fn unhandled_error(event: &Event) -> Option<Error> {
    if let Event::Row(row) = event {
        return Some(Error::invalid_configuration(format!(
            "the prompt has no row {row} to move to"
        )));
    }
    unhandled_outcome(event).map(|outcome| {
        if outcome == RunOutcome::Cancelled {
            Error::cancelled()
        } else {
            Error::input_ended()
        }
    })
}

#[cfg(test)]
mod tests {
    use bang_core::{
        ActionBinding,
        ActionLayer,
        widgets::{
            Select,
            TextInput,
        },
    };

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

    #[test]
    fn a_row_past_the_last_choice_fails_instead_of_wrapping() {
        let select = || Select::new("pick", ["a", "b", "c"]);
        let enter = Event::Key(KeyEvent::new(Key::Enter));

        assert_eq!(
            replay_events(select(), [Event::Row(2), enter.clone()]).unwrap(),
            bang_core::Value::from("c"),
        );
        assert_eq!(
            replay_events(select(), [Event::Row(3), enter])
                .unwrap_err()
                .kind(),
            crate::ErrorKind::InvalidConfiguration,
        );
    }
}
