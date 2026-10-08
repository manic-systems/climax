// SPDX-License-Identifier: EUPL-1.2

use crate::{Event, Value, WidgetId};

/// context from a handled event
#[derive(Debug, Default)]
pub struct Context {
    focus: Option<FocusTarget>,
}

impl Context {
    /// A context with no pending request.
    #[must_use]
    pub const fn new() -> Self {
        Self { focus: None }
    }

    /// Ask the session to move focus to `target` once the event is handled.
    pub fn request_focus(&mut self, target: FocusTarget) {
        self.focus = Some(target);
    }

    /// Take the pending focus request, leaving none.
    #[must_use]
    pub const fn take_focus(&mut self) -> Option<FocusTarget> {
        self.focus.take()
    }
}

/// focus request target
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FocusTarget {
    /// The widget with this id.
    Widget(WidgetId),
    /// The next focusable widget.
    Next,
    /// The previous focusable widget.
    Previous,
}

/// event handled result
#[derive(Clone, Debug, PartialEq)]
pub enum Reaction {
    /// The widget did not use the event.
    Ignored,
    /// State changed and the widget needs redrawing.
    Changed,
    /// The widget finished with a value.
    Submit(Value),
    /// action exit that bypasses enclosing containers until the session
    /// boundary
    Action(Value),
    /// The widget was cancelled.
    Cancel,
    /// Focus should move to the target.
    Focus(FocusTarget),
}

impl Reaction {
    /// Whether the event had any effect, which is everything except `Ignored`.
    #[must_use]
    pub fn changed(self) -> bool {
        matches!(
            self,
            Self::Changed | Self::Submit(_) | Self::Action(_) | Self::Cancel | Self::Focus(_)
        )
    }
}

/// An interactive `screw` widget.
///
/// Rendering comes from the [`screw::Widget`] supertrait, which draws into a
/// `screw` `Surface` and may place the terminal cursor on it. A list widget
/// learns how many rows it was given from `RenderCtx::available_rows` while
/// rendering, and keeps what it drew for the next call to `handle`.
pub trait Widget: screw::Widget {
    /// Identifies the widget to drivers and actions.
    fn id(&self) -> WidgetId;
    /// Handle `event`, using `cx` to request focus changes, and report what happened.
    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction;

    /// The value the widget would submit now, if it has one. Defaults to none.
    fn current_value(&self) -> Option<Value> {
        None
    }
}
