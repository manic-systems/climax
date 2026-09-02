use std::{cell::RefCell, fmt, rc::Rc};

use bang_core::{
    ActionBinding, ActionLayer, Context, Event, Reaction, Value, View, ViewContext, Widget,
    WidgetId,
};

use crate::{Error, Result};

type Runner = dyn Fn(Box<dyn Widget>) -> Result<Value>;
type GuardFactory = dyn Fn() -> Result<Box<dyn Guard>>;

trait Guard {}

impl<T> Guard for T {}

struct GuardStack(Vec<Box<dyn Guard>>);

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
    #[must_use]
    pub fn live() -> Self {
        Self::from_runner(|widget| {
            bang_screw::run_live_session(InteractionWidget::new(widget)).map_err(Error::from_live)
        })
    }

    /// Attempt a live session regardless of terminal capability detection.
    #[must_use]
    pub fn forced() -> Self {
        Self::from_runner(|widget| {
            bang_screw::run_live_session_forced(InteractionWidget::new(widget))
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
    /// This is used by presentation coordinators to suspend other transient
    /// output while a prompt owns the terminal region. Factories run in the
    /// order they were added; acquired guards are always released in reverse
    /// order, including when a later factory or the interaction itself fails.
    #[must_use]
    pub fn with_guard<G, F>(mut self, factory: F) -> Self
    where
        G: 'static,
        F: Fn() -> Result<G> + 'static,
    {
        self.guards.push(Rc::new(move || {
            factory().map(|guard| Box::new(guard) as Box<dyn Guard>)
        }));
        self
    }

    pub(crate) fn from_runner(runner: impl Fn(Box<dyn Widget>) -> Result<Value> + 'static) -> Self {
        Self {
            runner: Rc::new(runner),
            guards: Vec::new(),
        }
    }

    pub(crate) fn interact<W>(
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

impl Widget for InteractionWidget {
    fn id(&self) -> WidgetId {
        self.0.id()
    }

    fn handle(&mut self, event: Event, context: &mut Context) -> Reaction {
        self.0.handle(event, context)
    }

    fn view(&self, context: &ViewContext) -> View {
        self.0.view(context)
    }

    fn current_value(&self) -> Option<Value> {
        self.0.current_value()
    }
}

pub(crate) fn scripted(
    scripts: impl IntoIterator<Item = impl IntoIterator<Item = Event>>,
) -> Interaction {
    use std::collections::VecDeque;

    let scripts = scripts
        .into_iter()
        .map(|events| events.into_iter().collect::<Vec<_>>())
        .collect::<VecDeque<_>>();
    let scripts = Rc::new(RefCell::new(scripts));
    Interaction::from_runner(move |widget| {
        let events = scripts
            .try_borrow_mut()
            .map_err(|_error| Error::interaction_busy())?
            .pop_front()
            .ok_or_else(Error::input_ended)?;
        crate::advanced::replay_events(InteractionWidget::new(widget), events)
    })
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
