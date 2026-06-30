// SPDX-License-Identifier: EUPL-1.2

use std::collections::BTreeMap;

use crate::{
    Context,
    Event,
    FocusTarget,
    Key,
    Reaction,
    Role,
    Span,
    Value,
    View,
    ViewContext,
    Widget,
    WidgetId,
};

pub struct Form {
    id:     WidgetId,
    fields: Vec<FormField>,
    active: usize,
}

impl Form {
    #[must_use]
    pub fn new(id: impl Into<WidgetId>) -> Self {
        Self {
            id:     id.into(),
            fields: Vec::new(),
            active: 0,
        }
    }

    #[must_use]
    pub fn with_field(mut self, name: impl Into<String>, widget: impl Widget + 'static) -> Self {
        self.push_field(name, widget);
        self
    }

    pub fn push_field(&mut self, name: impl Into<String>, widget: impl Widget + 'static) {
        self.fields.push(FormField {
            name:   name.into(),
            widget: Box::new(widget),
        });
        self.active = self.active.min(self.fields.len().saturating_sub(1));
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.fields.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    #[must_use]
    pub const fn active_index(&self) -> usize {
        self.active
    }

    #[must_use]
    pub fn active_widget_id(&self) -> Option<WidgetId> {
        self.fields.get(self.active).map(|field| field.widget.id())
    }

    pub fn set_active_index(&mut self, active: usize) -> Reaction {
        if self.fields.is_empty() {
            self.active = 0;
            return Reaction::Ignored;
        }

        let active = active.min(self.fields.len() - 1);
        if active == self.active {
            return Reaction::Ignored;
        }
        self.active = active;
        self.focus_reaction()
    }

    fn move_focus(&mut self, delta: isize) -> Reaction {
        if self.fields.is_empty() {
            return Reaction::Ignored;
        }
        let len = self.fields.len();
        let next = self.active.saturating_add_signed(delta).min(len - 1);
        self.set_active_index(next)
    }

    fn focus_reaction(&self) -> Reaction {
        self.active_widget_id().map_or(Reaction::Changed, |id| {
            Reaction::Focus(FocusTarget::Widget(id))
        })
    }

    fn submit_or_advance(&mut self) -> Reaction {
        if self.fields.is_empty() || self.active + 1 == self.fields.len() {
            return Reaction::Submit(self.object_value());
        }
        self.move_focus(1)
    }

    fn object_value(&self) -> Value {
        Value::Object(
            self.fields
                .iter()
                .map(|field| {
                    (
                        field.name.clone(),
                        field.widget.current_value().unwrap_or(Value::Null),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        )
    }
}

impl Widget for Form {
    fn id(&self) -> WidgetId {
        self.id.clone()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        match &event {
            Event::Key(key) => {
                match key.key {
                    Key::Tab => return self.move_focus(1),
                    Key::Backtab => return self.move_focus(-1),
                    Key::Esc => return Reaction::Cancel,
                    _ => {},
                }
            },
            Event::Resize { .. } | Event::Tick | Event::Paste(_) => {},
        }

        let Some(field) = self.fields.get_mut(self.active) else {
            return match event {
                Event::Key(key) if key.key == Key::Enter => Reaction::Submit(self.object_value()),
                _ => Reaction::Ignored,
            };
        };

        match field.widget.handle(event, cx) {
            Reaction::Submit(_value) => self.submit_or_advance(),
            Reaction::Cancel => Reaction::Cancel,
            Reaction::Focus(FocusTarget::Next) => self.move_focus(1),
            Reaction::Focus(FocusTarget::Previous) => self.move_focus(-1),
            Reaction::Focus(FocusTarget::Widget(id)) => {
                self.fields
                    .iter()
                    .position(|field| field.widget.id() == id)
                    .map_or(Reaction::Focus(FocusTarget::Widget(id)), |index| {
                        self.set_active_index(index)
                    })
            },
            Reaction::Changed => Reaction::Changed,
            Reaction::Ignored => Reaction::Ignored,
        }
    }

    fn view(&self, cx: &ViewContext) -> View {
        let mut views = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            let active = index == self.active;
            views.push(View::Line(vec![
                Span::new(if active { "> " } else { "  " }, Role::Dim),
                Span::new(
                    field.name.clone(),
                    if active { Role::Selected } else { Role::Dim },
                ),
            ]));
            views.push(field.widget.view(cx));
        }

        if !self.fields.is_empty() {
            views.push(View::Line(vec![Span::new(
                "tab next | shift-tab previous | enter accept | esc cancel",
                Role::Dim,
            )]));
        }

        View::Stack(views)
    }

    fn current_value(&self) -> Option<Value> {
        Some(self.object_value())
    }
}

struct FormField {
    name:   String,
    widget: Box<dyn Widget>,
}
