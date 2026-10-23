// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use bang::{
    advanced::{
        Event,
        Key,
        KeyEvent,
        Reaction,
        Value,
        Widget,
        WidgetContext,
        WidgetId,
        replay_events,
    },
    screw::{
        RenderCtx,
        Role,
        Surface,
        Widget as ScreenWidget,
    },
};

struct Echo {
    typed: String,
}

impl ScreenWidget for Echo {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        out.write(&self.typed, ctx.theme().style(Role::Prompt));
    }
}

impl Widget for Echo {
    fn id(&self) -> WidgetId {
        WidgetId::from("echo")
    }

    fn handle(&mut self, event: Event, _cx: &mut WidgetContext) -> Reaction {
        match event {
            Event::Key(KeyEvent {
                key: Key::Char(value),
                ..
            }) => {
                self.typed.push(value);
                Reaction::Changed
            },
            Event::Key(KeyEvent {
                key: Key::Enter, ..
            }) => Reaction::Submit(Value::from(self.typed.clone())),
            Event::Key(_)
            | Event::Paste(_)
            | Event::UnknownEscape(_)
            | Event::Resize { .. }
            | Event::Tick
            | _ => Reaction::Ignored,
        }
    }
}

#[test]
fn a_widget_is_implementable_through_advanced_and_screw() {
    let value = replay_events(
        Echo {
            typed: String::new(),
        },
        [
            Event::Key(KeyEvent::new(Key::Char('h'))),
            Event::Key(KeyEvent::new(Key::Char('i'))),
            Event::Key(KeyEvent::new(Key::Enter)),
        ],
    )
    .unwrap();

    assert_eq!(value, Value::from("hi"));
}
