// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Event,
    Value,
    WidgetId,
};

/// the context a widget handles an event in, it carries nothing yet
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Context;

impl Context {
    /// an empty context
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// focus request target
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FocusTarget {
    /// the widget with this id
    Widget(WidgetId),
    /// the next focusable widget
    Next,
    /// the previous focusable widget
    Previous,
}

/// event handled result
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Reaction {
    /// the widget did not use the event
    Ignored,
    /// state changed and the widget needs redrawing
    Changed,
    /// the widget finished with a value
    Submit(Value),
    /// action exit that bypasses enclosing containers until the session
    /// boundary
    Action(Value),
    /// the widget was cancelled
    Cancel,
    /// focus should move to the target
    Focus(FocusTarget),
}

impl Reaction {
    /// whether the event had any effect, everything except `Ignored`
    #[must_use]
    pub fn changed(self) -> bool {
        matches!(
            self,
            Self::Changed | Self::Submit(_) | Self::Action(_) | Self::Cancel | Self::Focus(_)
        )
    }
}

/// an interactive `screw` widget
pub trait Widget: screw::Widget {
    /// identifies the widget to drivers and actions
    fn id(&self) -> WidgetId;
    /// handles `event` and reports what happened
    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction;

    /// whether the last line a widget renders is a key help line, which an
    /// enclosing [`ActionLayer`](crate::ActionLayer) extends, false by default
    fn has_key_help(&self) -> bool {
        false
    }

    /// whether the widget uses Tab itself, so a [`Form`](crate::widgets::Form)
    /// cannot move focus with it, false by default
    fn takes_tab(&self) -> bool {
        false
    }

    /// whether the widget has nothing to pick and so can never submit, which a
    /// [`Form`](crate::widgets::Form) refuses as a field, false by default
    fn offers_nothing(&self) -> bool {
        false
    }

    /// the value the widget would submit now, none by default
    fn current_value(&self) -> Option<Value> {
        None
    }
}
