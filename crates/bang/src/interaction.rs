// SPDX-License-Identifier: EUPL-1.2

use std::{any::Any, cell::RefCell, collections::VecDeque, fmt, os::fd::OwnedFd, rc::Rc};

use bang_core::{
    ActionBinding, ActionLayer, Context, Event, Reaction, Value, Widget, WidgetId,
};
use screw::{RenderCtx, Surface, TickInterest, VerticalSize};

use crate::{Error, Result};

type Runner = dyn Fn(Box<dyn Widget>) -> Result<Value>;
type GuardFactory = dyn Fn() -> Result<Box<dyn Any>>;

struct GuardStack(Vec<Box<dyn Any>>);

impl Drop for GuardStack {
    fn drop(&mut self) {
        while self.0.pop().is_some() {}
    }
}

/// A cloneable driver for typed prompt interactions.
///
/// The default driver uses the process terminal. Alternative drivers can be
/// supplied by application policy or deterministic tests without changing the
/// typed prompt API.
#[derive(Clone)]
pub struct Interaction {
    runner: Rc<Runner>,
    guards: Vec<Rc<GuardFactory>>,
}

impl Interaction {
    /// Use stdin and stderr when both support an interactive terminal session.
    ///
    /// Resolves to `ErrorKind::InteractionUnavailable` when either stream is
    /// not a terminal, or when `TERM` is `dumb`.
    #[must_use]
    pub fn live() -> Self {
        Self::from_runner(|widget| {
            crate::live::run_live_session(InteractionWidget::new(widget)).map_err(Error::from_live)
        })
    }

    /// Attempt a live session regardless of terminal capability detection.
    #[must_use]
    pub fn forced() -> Self {
        Self::from_runner(|widget| {
            crate::live::run_live_session_forced(InteractionWidget::new(widget))
                .map_err(Error::from_live)
        })
    }

    /// Use a caller-owned terminal handle, such as `/dev/tty`, when it
    /// supports an interactive terminal session.
    ///
    /// Each prompt duplicates `handle` for its own session, because the
    /// runner underneath runs once per prompt and needs a handle it can
    /// consume.
    #[must_use]
    pub fn live_on(handle: impl Into<OwnedFd>) -> Self {
        let handle = handle.into();
        Self::from_runner(move |widget| {
            let handle = handle.try_clone().map_err(Error::terminal)?;
            crate::live::run_live_session_on(InteractionWidget::new(widget), handle)
                .map_err(Error::from_live)
        })
    }

    /// Attempt a live session on a caller-owned terminal handle regardless of
    /// terminal capability detection.
    #[must_use]
    pub fn forced_on(handle: impl Into<OwnedFd>) -> Self {
        let handle = handle.into();
        Self::from_runner(move |widget| {
            let handle = handle.try_clone().map_err(Error::terminal)?;
            crate::live::run_live_session_forced_on(InteractionWidget::new(widget), handle)
                .map_err(Error::from_live)
        })
    }

    /// Reject prompt interaction without touching the terminal.
    #[must_use]
    pub fn disabled() -> Self {
        Self::from_runner(|_widget| Err(Error::interaction_unavailable()))
    }

    /// Acquire an application-owned guard around each interaction.
    ///
    /// A guard can pause other terminal output or take a lock for as long as a
    /// prompt owns the terminal. Factories run in the order they were added.
    /// Acquired guards are released in reverse order, including when a later
    /// factory or the interaction itself fails.
    #[must_use]
    pub fn with_guard<G, F>(mut self, factory: F) -> Self
    where
        G: 'static,
        F: Fn() -> Result<G> + 'static,
    {
        self.guards.push(Rc::new(move || {
            factory().map(|guard| Box::new(guard) as Box<dyn Any>)
        }));
        self
    }

    pub(crate) fn interact_named<W>(
        &self,
        prompt: Option<&str>,
        widget: W,
        actions: impl IntoIterator<Item = ActionBinding>,
    ) -> Result<Value>
    where
        W: Widget + 'static,
    {
        self.interact(widget, actions)
            .map_err(|error| match prompt {
                Some(prompt) => error.naming_prompt(prompt),
                None => error,
            })
    }

    pub(crate) fn from_runner(runner: impl Fn(Box<dyn Widget>) -> Result<Value> + 'static) -> Self {
        Self {
            runner: Rc::new(runner),
            guards: Vec::new(),
        }
    }

    /// Run a custom widget on this driver, with `actions` bound as extra keys.
    ///
    /// This is how a widget built from [`crate::advanced`] reaches a driver
    /// other than the process terminal, such as [`Self::live_on`] with
    /// `/dev/tty` or [`Self::forced`]. Application guards run around it.
    pub fn interact<W>(
        &self,
        widget: W,
        actions: impl IntoIterator<Item = ActionBinding>,
    ) -> Result<Value>
    where
        W: Widget + 'static,
    {
        let mut guards = GuardStack(Vec::with_capacity(self.guards.len()));
        for factory in &self.guards {
            guards.0.push(factory()?);
        }
        let widget = ActionLayer::new(widget).with_actions(actions);
        let result = (self.runner)(Box::new(widget));
        drop(guards);
        result
    }
}

impl Default for Interaction {
    fn default() -> Self {
        Self::live()
    }
}

impl fmt::Debug for Interaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interaction")
            .field("guards", &self.guards.len())
            .finish_non_exhaustive()
    }
}

/// A type-erased widget supplied to an advanced interaction runner.
pub struct InteractionWidget(Box<dyn Widget>);

impl InteractionWidget {
    pub(crate) fn new(widget: Box<dyn Widget>) -> Self {
        Self(widget)
    }
}

impl screw::Widget for InteractionWidget {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.0.render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.0.tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.0.vertical_size()
    }
}

impl Widget for InteractionWidget {
    fn id(&self) -> WidgetId {
        self.0.id()
    }

    fn handle(&mut self, event: Event, context: &mut Context) -> Reaction {
        self.0.handle(event, context)
    }

    fn current_value(&self) -> Option<Value> {
        self.0.current_value()
    }
}

pub(crate) fn scripted(
    scripts: impl IntoIterator<Item = impl IntoIterator<Item = Event>>,
) -> Interaction {
    let scripts = scripts
        .into_iter()
        .map(|events| events.into_iter().collect::<Vec<_>>())
        .collect();
    let state = Rc::new(RefCell::new(Script {
        scripts,
        unused_events: 0,
    }));
    Interaction::from_runner(move |widget| {
        let events = state
            .borrow_mut()
            .scripts
            .pop_front()
            .ok_or_else(Error::input_ended)?;
        let mut events = events.into_iter();
        let result = crate::advanced::replay_events(InteractionWidget::new(widget), &mut events);
        state.borrow_mut().unused_events += events.count();
        result
    })
}

struct Script {
    scripts: VecDeque<Vec<Event>>,
    unused_events: usize,
}

impl Drop for Script {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        assert!(
            self.scripts.is_empty() && self.unused_events == 0,
            "scripted interaction dropped with {} unused scripts and {} unused events",
            self.scripts.len(),
            self.unused_events,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordedGuard {
        label: &'static str,
        events: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Drop for RecordedGuard {
        fn drop(&mut self) {
            self.events.borrow_mut().push(self.label);
        }
    }

    #[test]
    #[should_panic(expected = "1 unused scripts and 0 unused events")]
    fn dropping_a_scripted_interaction_with_an_unstarted_script_panics() {
        let _ = scripted([Vec::new(), Vec::new()]).interact(bang_core::widgets::TextInput::new("w"), []);
    }

    #[test]
    #[should_panic(expected = "0 unused scripts and 1 unused events")]
    fn dropping_a_scripted_interaction_with_unread_events_panics() {
        let submit = Event::Key(bang_core::KeyEvent::new(bang_core::Key::Enter));
        let interaction = scripted([[submit.clone(), submit]]);
        interaction
            .interact(bang_core::widgets::TextInput::new("w"), [])
            .unwrap();
    }

    #[test]
    fn a_fully_consumed_scripted_interaction_drops_quietly() {
        let submit = Event::Key(bang_core::KeyEvent::new(bang_core::Key::Enter));
        scripted([[submit]])
            .interact(bang_core::widgets::TextInput::new("w"), [])
            .unwrap();
    }

    #[test]
    fn guards_are_acquired_in_order_and_released_lifo() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let runner_events = events.clone();
        let interaction = Interaction::from_runner(move |_widget| {
            runner_events.borrow_mut().push("run");
            Ok(Value::from("done"))
        });
        let first_events = events.clone();
        let interaction = interaction.with_guard(move || {
            first_events.borrow_mut().push("acquire first");
            Ok(RecordedGuard {
                label: "release first",
                events: first_events.clone(),
            })
        });
        let second_events = events.clone();
        let interaction = interaction.with_guard(move || {
            second_events.borrow_mut().push("acquire second");
            Ok(RecordedGuard {
                label: "release second",
                events: second_events.clone(),
            })
        });

        interaction
            .interact(bang_core::widgets::TextInput::new("widget"), [])
            .unwrap();

        assert_eq!(
            *events.borrow(),
            [
                "acquire first",
                "acquire second",
                "run",
                "release second",
                "release first",
            ]
        );
    }

    #[test]
    fn acquired_guards_unwind_lifo_when_later_acquisition_fails() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let first_events = events.clone();
        let interaction = Interaction::disabled().with_guard(move || {
            first_events.borrow_mut().push("acquire first");
            Ok(RecordedGuard {
                label: "release first",
                events: first_events.clone(),
            })
        });
        let second_events = events.clone();
        let interaction = interaction.with_guard(move || -> Result<RecordedGuard> {
            second_events.borrow_mut().push("acquire second");
            Err(Error::interaction_unavailable())
        });

        let error = interaction
            .interact(bang_core::widgets::TextInput::new("widget"), [])
            .unwrap_err();

        assert_eq!(error.kind(), crate::ErrorKind::InteractionUnavailable);
        assert_eq!(
            *events.borrow(),
            ["acquire first", "acquire second", "release first"]
        );
    }
}
