// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Context, Event, FocusTarget, Presentation, Reaction, Value, View, ViewContext, Widget,
};

pub struct Session {
    root: Box<dyn Widget>,
    focus: Option<FocusTarget>,
    status: SessionStatus,
    view_context: ViewContext,
    presentation: Presentation,
    dirty: bool,
}

impl Session {
    #[must_use]
    pub fn new(root: impl Widget + 'static) -> Self {
        Self {
            root: Box::new(root),
            focus: None,
            status: SessionStatus::Running,
            view_context: ViewContext::default(),
            presentation: Presentation::default(),
            dirty: true,
        }
    }

    #[must_use]
    pub fn boxed(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            status: SessionStatus::Running,
            view_context: ViewContext::default(),
            presentation: Presentation::default(),
            dirty: true,
        }
    }

    pub fn handle(&mut self, event: Event) -> Reaction {
        if !matches!(self.status, SessionStatus::Running) {
            return Reaction::Ignored;
        }

        let resized = matches!(event, Event::Resize { .. });
        let mut context = Context::with_presentation(self.presentation.clone());
        let handled = self.root.handle(event, &mut context);
        let reaction = context
            .take_focus()
            .map_or_else(|| resize_reaction(handled, resized), Reaction::Focus);

        match &reaction {
            Reaction::Ignored => {},
            Reaction::Changed => {
                self.dirty = true;
            },
            Reaction::Submit(value) => {
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

        reaction
    }

    #[must_use]
    pub fn view(&self) -> View {
        self.root.view(&self.view_context)
    }

    #[must_use]
    pub const fn view_context(&self) -> &ViewContext {
        &self.view_context
    }

    #[must_use]
    pub const fn status(&self) -> &SessionStatus {
        &self.status
    }

    #[must_use]
    pub const fn focus(&self) -> Option<&FocusTarget> {
        self.focus.as_ref()
    }

    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub const fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn set_presentation(&mut self, presentation: Presentation) {
        self.presentation = presentation;
    }
}

fn resize_reaction(reaction: Reaction, resized: bool) -> Reaction {
    if resized && matches!(reaction, Reaction::Ignored) {
        Reaction::Changed
    } else {
        reaction
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionStatus {
    Running,
    Submitted(Value),
    Cancelled,
}
