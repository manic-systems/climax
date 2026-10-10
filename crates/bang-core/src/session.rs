// SPDX-License-Identifier: EUPL-1.2

use screw::{
    RenderCtx,
    Surface,
    TickInterest,
    VerticalSize,
};

use crate::{
    Context,
    Event,
    FocusTarget,
    Reaction,
    Value,
    Widget,
};

/// Runs one root widget and tracks whether it has finished and needs redrawing.
pub struct Session {
    root:   Box<dyn Widget>,
    focus:  Option<FocusTarget>,
    status: SessionStatus,
    dirty:  bool,
}

impl Session {
    /// Start a session around `root`.
    #[must_use]
    pub fn new(root: impl Widget + 'static) -> Self {
        Self {
            root:   Box::new(root),
            focus:  None,
            status: SessionStatus::Running,
            dirty:  true,
        }
    }

    /// Start a session around an already boxed `root`.
    #[must_use]
    pub fn boxed(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            status: SessionStatus::Running,
            dirty: true,
        }
    }

    /// Deliver `event` to the root and record the outcome.
    ///
    /// A finished session ignores further events. A focus request made while
    /// handling the event only takes effect when the widget did not also
    /// submit or cancel.
    pub fn handle(&mut self, event: Event) -> SessionReaction {
        if !matches!(self.status, SessionStatus::Running) {
            return SessionReaction::Ignored;
        }

        let redraws = match event {
            Event::Resize { .. } => true,
            Event::Tick => !matches!(self.root.tick_interest(), TickInterest::Never),
            _ => false,
        };
        let mut context = Context::new();
        let handled = self.root.handle(event, &mut context);
        let reaction = match (handled, context.take_focus()) {
            (terminal @ (Reaction::Submit(_) | Reaction::Action(_) | Reaction::Cancel), _) => {
                terminal
            },
            (_, Some(target)) => Reaction::Focus(target),
            (handled, None) => redraw_reaction(handled, redraws),
        };

        match &reaction {
            Reaction::Ignored => {},
            Reaction::Changed => {
                self.dirty = true;
            },
            Reaction::Submit(value) | Reaction::Action(value) => {
                self.status = SessionStatus::Submitted(value.clone());
                self.dirty = true;
            },
            Reaction::Cancel => {
                self.status = SessionStatus::Cancelled;
                self.dirty = true;
            },
            Reaction::Focus(target) => {
                self.focus = Some(target.clone());
                self.dirty = true;
            },
        }

        // containers route actions to the session boundary; drivers only ever
        // see a submission
        match reaction {
            Reaction::Ignored => SessionReaction::Ignored,
            Reaction::Changed => SessionReaction::Changed,
            Reaction::Submit(value) | Reaction::Action(value) => SessionReaction::Submit(value),
            Reaction::Cancel => SessionReaction::Cancel,
            Reaction::Focus(target) => SessionReaction::Focus(target),
        }
    }

    /// Whether the session is running or how it ended.
    #[must_use]
    pub const fn status(&self) -> &SessionStatus {
        &self.status
    }

    /// The latest focus request, if a widget made one.
    #[must_use]
    pub const fn focus(&self) -> Option<&FocusTarget> {
        self.focus.as_ref()
    }

    /// Whether the session changed since the last [`Self::clear_dirty`].
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the current state as drawn.
    pub const fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

impl screw::Widget for Session {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.root.render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.root.tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.root.vertical_size()
    }
}

/// A resize, or a tick for a widget that asked for them, always needs a redraw.
fn redraw_reaction(reaction: Reaction, redraws: bool) -> Reaction {
    if redraws && matches!(reaction, Reaction::Ignored) {
        Reaction::Changed
    } else {
        reaction
    }
}

/// How far a [`Session`] has got.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionStatus {
    /// Still waiting for input.
    Running,
    /// Finished with this value.
    Submitted(Value),
    /// Finished without a value.
    Cancelled,
}

/// What a driver sees back from [`Session::handle`].
///
/// Unlike [`Reaction`], there is no `Action` variant: the session already
/// rewrote it into `Submit` before returning, so a driver has no exhaustive
/// arm to keep in sync with a variant it can never receive.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionReaction {
    /// The root did not use the event.
    Ignored,
    /// State changed and needs redrawing.
    Changed,
    /// The session finished with this value.
    Submit(Value),
    /// The session was cancelled.
    Cancel,
    /// Focus should move to the target.
    Focus(FocusTarget),
}

impl SessionReaction {
    /// Whether the event had any effect, which is everything except `Ignored`.
    #[must_use]
    pub fn changed(self) -> bool {
        matches!(
            self,
            Self::Changed | Self::Submit(_) | Self::Cancel | Self::Focus(_)
        )
    }
}
