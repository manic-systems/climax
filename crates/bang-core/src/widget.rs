// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Event,
    Value,
    WidgetId,
};

/// context from a handled event
#[derive(Debug, Default)]
pub struct Context {
    focus: Option<FocusTarget>,
}

impl Context {
    /// a context with no pending request
    #[must_use]
    pub const fn new() -> Self {
        Self { focus: None }
    }

    /// asks the session to move focus to `target` once the event is handled
    pub fn request_focus(&mut self, target: FocusTarget) {
        self.focus = Some(target);
    }

    /// takes the pending focus request, leaving none
    #[must_use]
    pub const fn take_focus(&mut self) -> Option<FocusTarget> {
        self.focus.take()
    }
}

/// focus request target
#[derive(Clone, Debug, Eq, PartialEq)]
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
    /// handles `event`, using `cx` to request focus changes, and reports what
    /// happened
    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction;

    /// the value the widget would submit now, none by default
    fn current_value(&self) -> Option<Value> {
        None
    }
}
