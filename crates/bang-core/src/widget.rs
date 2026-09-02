// SPDX-License-Identifier: EUPL-1.2

use crate::{Event, ListPresentation, Presentation, Value, View, ViewContext, ViewId, WidgetId};

/// context from a handled event
#[derive(Debug, Default)]
pub struct Context {
    focus: Option<FocusTarget>,
    presentation: Presentation,
}

impl Context {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            focus: None,
            presentation: Presentation { lists: Vec::new() },
        }
    }

    #[must_use]
    pub const fn with_presentation(presentation: Presentation) -> Self {
        Self {
            focus: None,
            presentation,
        }
    }

    pub fn request_focus(&mut self, target: FocusTarget) {
        self.focus = Some(target);
    }

    #[must_use]
    pub const fn take_focus(&mut self) -> Option<FocusTarget> {
        self.focus.take()
    }

    #[must_use]
    pub fn list_presentation(&self, id: &ViewId) -> Option<&ListPresentation> {
        self.presentation.list(id)
    }
}

/// focus request target
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FocusTarget {
    Widget(WidgetId),
    Next,
    Previous,
}

/// event handled result
#[derive(Clone, Debug, PartialEq)]
pub enum Reaction {
    Ignored,
    Changed,
    Submit(Value),
    Cancel,
    Focus(FocusTarget),
}

impl Reaction {
    #[must_use]
    pub fn changed(self) -> bool {
        matches!(
            self,
            Self::Changed | Self::Submit(_) | Self::Cancel | Self::Focus(_)
        )
    }
}

pub trait Widget {
    fn id(&self) -> WidgetId;
    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction;
    fn view(&self, cx: &ViewContext) -> View;

    fn current_value(&self) -> Option<Value> {
        None
    }
}
