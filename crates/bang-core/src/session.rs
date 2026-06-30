// SPDX-License-Identifier: EUPL-1.2

use crate::{
    Context,
    Event,
    FocusTarget,
    Reaction,
    Value,
    View,
    ViewContext,
    Widget,
};

pub struct Session {
    root:         Box<dyn Widget>,
    focus:        Option<FocusTarget>,
    status:       SessionStatus,
    view_context: ViewContext,
    dirty:        bool,
}

impl Session {
    #[must_use]
    pub fn new(root: impl Widget + 'static) -> Self {
        Self {
            root:         Box::new(root),
            focus:        None,
            status:       SessionStatus::Running,
            view_context: ViewContext::default(),
            dirty:        true,
        }
    }

    #[must_use]
    pub fn boxed(root: Box<dyn Widget>) -> Self {
        Self {
            root,
            focus: None,
            status: SessionStatus::Running,
            view_context: ViewContext::default(),
            dirty: true,
        }
    }

    pub fn handle(&mut self, event: Event) -> Reaction {
        if !matches!(self.status, SessionStatus::Running) {
            return Reaction::Ignored;
        }

        let resized = self.record_resize(&event);
        let mut context = Context::new();
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

    fn record_resize(&mut self, event: &Event) -> bool {
        let Event::Resize { cols, rows } = event else {
            return false;
        };

        let width = Some(*cols);
        let height = Some(*rows);
        if self.view_context.width == width && self.view_context.height == height {
            return false;
        }

        self.view_context = ViewContext { width, height };
        true
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

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
    };

    use super::Session;
    use crate::{
        Context,
        Event,
        Reaction,
        View,
        Widget,
        WidgetId,
    };

    #[test]
    fn resize_updates_view_context_and_marks_session_dirty() {
        let seen = Rc::new(RefCell::new(None));
        let mut session = Session::new(ProbeWidget {
            seen: Rc::clone(&seen),
        });
        session.clear_dirty();

        assert_eq!(
            session.handle(Event::Resize {
                cols: 100,
                rows: 30,
            }),
            Reaction::Changed
        );
        assert!(session.is_dirty());
        assert_eq!(session.view_context().width, Some(100));
        assert_eq!(session.view_context().height, Some(30));

        let _view = session.view();
        assert_eq!(*seen.borrow(), Some((Some(100), Some(30))));
    }

    #[test]
    fn same_resize_is_ignored_when_widget_ignores_it() {
        let seen = Rc::new(RefCell::new(None));
        let mut session = Session::new(ProbeWidget { seen });

        assert_eq!(
            session.handle(Event::Resize { cols: 80, rows: 24 }),
            Reaction::Changed
        );
        session.clear_dirty();

        assert_eq!(
            session.handle(Event::Resize { cols: 80, rows: 24 }),
            Reaction::Ignored
        );
        assert!(!session.is_dirty());
    }

    type SeenContext = Rc<RefCell<Option<(Option<u16>, Option<u16>)>>>;

    struct ProbeWidget {
        seen: SeenContext,
    }

    impl Widget for ProbeWidget {
        fn id(&self) -> WidgetId {
            "probe".into()
        }

        fn handle(&mut self, _event: Event, _cx: &mut Context) -> Reaction {
            Reaction::Ignored
        }

        fn view(&self, cx: &crate::ViewContext) -> View {
            *self.seen.borrow_mut() = Some((cx.width, cx.height));
            View::Empty
        }
    }
}
