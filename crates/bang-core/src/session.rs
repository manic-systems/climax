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

/// runs one root widget and tracks whether it has finished and needs redrawing
pub struct Session {
    root:   Box<dyn Widget>,
    focus:  Option<FocusTarget>,
    status: SessionStatus,
    dirty:  bool,
}

impl Session {
    /// starts a session around `root`
    #[must_use]
    pub fn new(root: impl Widget + 'static) -> Self {
        Self {
            root:   Box::new(root),
            focus:  None,
            status: SessionStatus::Running,
            dirty:  true,
        }
    }

    /// starts a session around an already boxed `root`
    #[must_use]
    pub fn boxed(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            status: SessionStatus::Running,
            dirty: true,
        }
    }

    /// delivers `event` to the root and records the outcome, a finished session
    /// ignores events
    pub fn handle(&mut self, event: Event) -> SessionReaction {
        if !matches!(self.status, SessionStatus::Running) {
            return SessionReaction::Ignored;
        }

        let redraws = match event {
            Event::Resize { .. } => true,
            Event::Tick => !matches!(self.root.tick_interest(), TickInterest::Never),
            Event::Key(_) | Event::Paste(_) | Event::UnknownEscape(_) => false,
        };
        let mut context = Context::new();
        let handled = self.root.handle(event, &mut context);
        let reaction = match (handled, context.take_focus()) {
            (terminal @ (Reaction::Submit(_) | Reaction::Action(_) | Reaction::Cancel), _) => {
                terminal
            },
            (_, Some(target)) => Reaction::Focus(target),
            (other, None) => redraw_reaction(other, redraws),
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

        // containers route actions to the session boundary, drivers only ever
        // see a submission
        match reaction {
            Reaction::Ignored => SessionReaction::Ignored,
            Reaction::Changed => SessionReaction::Changed,
            Reaction::Submit(value) | Reaction::Action(value) => SessionReaction::Submit(value),
            Reaction::Cancel => SessionReaction::Cancel,
            Reaction::Focus(target) => SessionReaction::Focus(target),
        }
    }

    /// whether the session is running or how it ended
    #[must_use]
    pub const fn status(&self) -> &SessionStatus {
        &self.status
    }

    /// the latest focus request, if a widget made one
    #[must_use]
    pub const fn focus(&self) -> Option<&FocusTarget> {
        self.focus.as_ref()
    }

    /// whether the session changed since the last [`Self::clear_dirty`]
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// marks the current state as drawn
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

/// a resize, or a tick for a widget that asked for them, always needs a redraw
fn redraw_reaction(reaction: Reaction, redraws: bool) -> Reaction {
    if redraws && matches!(reaction, Reaction::Ignored) {
        Reaction::Changed
    } else {
        reaction
    }
}

/// how far a [`Session`] has got
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SessionStatus {
    /// still waiting for input
    Running,
    /// finished with this value
    Submitted(Value),
    /// finished without a value
    Cancelled,
}

/// what a driver sees back from [`Session::handle`], [`Reaction`] without the
/// `Action` variant
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SessionReaction {
    /// the root did not use the event
    Ignored,
    /// state changed and needs redrawing
    Changed,
    /// the session finished with this value
    Submit(Value),
    /// the session was cancelled
    Cancel,
    /// focus should move to the target
    Focus(FocusTarget),
}

impl SessionReaction {
    /// whether the event had any effect, everything except `Ignored`
    #[must_use]
    pub fn changed(self) -> bool {
        matches!(
            self,
            Self::Changed | Self::Submit(_) | Self::Cancel | Self::Focus(_)
        )
    }
}
