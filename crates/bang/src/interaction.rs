// SPDX-License-Identifier: EUPL-1.2

use alloc::{
    collections::VecDeque,
    rc::Rc,
};
use core::{
    any::Any,
    cell::RefCell,
    fmt,
    iter::Peekable,
    str::Chars,
};
use std::{
    fs::File,
    io::{
        self,
        IsTerminal as _,
    },
    os::fd::OwnedFd,
    thread,
};

use bang_core::{
    ActionBinding,
    ActionLayer,
    Context,
    Event,
    Reaction,
    Value,
    Widget,
    WidgetId,
};
use screw::{
    RenderCtx,
    Surface,
    TickInterest,
    VerticalSize,
};

use crate::{
    Error,
    Result,
    advanced::replay_events,
    live::{
        run_live_session,
        run_live_session_forced,
        run_live_session_forced_on,
        run_live_session_on,
    },
};

type Runner = dyn Fn(Box<dyn Widget>) -> Result<Value>;
type GuardFactory = dyn Fn() -> Result<Box<dyn Any>>;
type SummarySink = dyn Fn(&str) -> io::Result<()>;
type SummaryAnswer<'a> = &'a dyn Fn(&Value) -> Option<String>;
type Abandon = dyn Fn();

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn write_summary_line(mut out: impl io::Write, line: &str) -> io::Result<()> {
    if screw::colors_enabled() {
        writeln!(out, "{DIM}{line}{RESET}")?;
    } else {
        writeln!(out, "{line}")?;
    }
    out.flush()
}

fn stderr_sink() -> Rc<SummarySink> {
    Rc::new(|line| {
        let stderr = io::stderr();
        if !stderr.is_terminal() {
            return Ok(());
        }
        write_summary_line(stderr.lock(), line)
    })
}

fn handle_sink(handle: &OwnedFd) -> io::Result<Rc<SummarySink>> {
    let file = File::from(handle.try_clone()?);
    Ok(Rc::new(move |line| {
        if !file.is_terminal() {
            return Ok(());
        }
        write_summary_line(&file, line)
    }))
}

struct GuardStack(Vec<Box<dyn Any>>);

impl Drop for GuardStack {
    fn drop(&mut self) {
        while self.0.pop().is_some() {}
    }
}

/// a cloneable driver for typed prompt interactions
///
/// the default driver uses the process terminal, and other drivers come from
/// application policy or deterministic tests
#[derive(Clone)]
pub struct Interaction {
    runner:       Rc<Runner>,
    guards:       Vec<Rc<GuardFactory>>,
    summary_sink: Option<Rc<SummarySink>>,
    summaries:    bool,
    abandon:      Option<Rc<Abandon>>,
}

impl Interaction {
    /// use stdin and stderr when both are terminals
    ///
    /// resolves to `ErrorKind::InteractionUnavailable` when either is not, or
    /// `TERM` is `dumb`
    #[must_use]
    pub fn live() -> Self {
        Self::from_runner(|widget| {
            run_live_session(InteractionWidget::new(widget)).map_err(Error::from_live)
        })
        .with_summary_sink(stderr_sink())
    }

    /// attempt a live session without checking terminal capability
    #[must_use]
    pub fn forced() -> Self {
        Self::from_runner(|widget| {
            run_live_session_forced(InteractionWidget::new(widget)).map_err(Error::from_live)
        })
        .with_summary_sink(stderr_sink())
    }

    /// use a caller-owned terminal handle such as `/dev/tty`
    ///
    /// each prompt duplicates `handle`, since the runner underneath consumes
    /// one per prompt
    #[must_use]
    pub fn live_on(handle: impl Into<OwnedFd>) -> Self {
        let owned = handle.into();
        let sink = handle_sink(&owned);
        Self::from_runner(move |widget| {
            let duplicate = owned.try_clone().map_err(Error::terminal)?;
            run_live_session_on(InteractionWidget::new(widget), duplicate).map_err(Error::from_live)
        })
        .with_optional_summary_sink(sink.ok())
    }

    /// attempt a live session on a caller-owned handle without checking
    /// terminal capability
    #[must_use]
    pub fn forced_on(handle: impl Into<OwnedFd>) -> Self {
        let owned = handle.into();
        let sink = handle_sink(&owned);
        Self::from_runner(move |widget| {
            let duplicate = owned.try_clone().map_err(Error::terminal)?;
            run_live_session_forced_on(InteractionWidget::new(widget), duplicate)
                .map_err(Error::from_live)
        })
        .with_optional_summary_sink(sink.ok())
    }

    /// reject prompt interaction without touching the terminal
    #[must_use]
    pub fn disabled() -> Self {
        Self::from_runner(|_widget| Err(Error::interaction_unavailable()))
    }

    /// acquire an application-owned guard around each interaction, such as one
    /// that pauses other terminal output
    ///
    /// factories run in the order added and guards release in reverse,
    /// including when a later factory or the interaction fails
    #[must_use]
    pub fn with_guard<G, F>(mut self, factory: F) -> Self
    where
        G: 'static,
        F: Fn() -> Result<G> + 'static,
    {
        self.guards.push(Rc::new(move || {
            factory().map(|guard| -> Box<dyn Any> { Box::new(guard) })
        }));
        self
    }

    /// whether a submitted prompt leaves a one-line summary in the scrollback
    /// such as `Deploy to prod? › no`, on by default
    ///
    /// only live terminal drivers write summaries, dimmed unless `NO_COLOR` is
    /// set. leaving writes nothing, and neither does a prompt with no
    /// header or prompt text to label the answer. a prompt's own `summary`
    /// setting wins
    #[must_use]
    pub const fn with_summaries(mut self, summaries: bool) -> Self {
        self.summaries = summaries;
        self
    }

    fn with_summary_sink(self, sink: Rc<SummarySink>) -> Self {
        self.with_optional_summary_sink(Some(sink))
    }

    fn with_optional_summary_sink(mut self, sink: Option<Rc<SummarySink>>) -> Self {
        self.summary_sink = sink;
        self
    }

    pub(crate) fn interact_named<W>(
        &self,
        prompt: Option<&str>,
        widget: W,
        actions: impl IntoIterator<Item = ActionBinding>,
        summary: Summary<'_>,
    ) -> Result<Value>
    where
        W: Widget + 'static,
    {
        let heading = prompt.map(summary_label).filter(|text| !text.is_empty());
        let line = |value: &Value| {
            let label = heading.as_deref()?;
            let answer = (summary.answer)(value)?;
            let line = if answer.is_empty() {
                label.to_owned()
            } else {
                format!("{label} › {answer}")
            };
            Some(sanitised_line(&line))
        };
        let enabled = summary.enabled.unwrap_or(self.summaries);
        let answer_line: SummaryAnswer<'_> = &line;
        self.run(widget, actions, enabled.then_some(answer_line))
            .map_err(|error| {
                match prompt {
                    Some(name) => error.naming_prompt(name),
                    None => error,
                }
            })
    }

    pub(crate) fn from_runner(runner: impl Fn(Box<dyn Widget>) -> Result<Value> + 'static) -> Self {
        Self {
            runner:       Rc::new(runner),
            guards:       Vec::new(),
            summary_sink: None,
            summaries:    true,
            abandon:      None,
        }
    }

    pub(crate) fn rejected(&self, error: Error) -> Error {
        if let Some(abandon) = &self.abandon {
            abandon();
        }
        error
    }

    /// run a custom widget on this driver, with `actions` bound as extra keys
    ///
    /// how a widget built from [`crate::advanced`] reaches a driver other than
    /// the process terminal, such as [`Self::live_on`] or [`Self::forced`]
    ///
    /// # Errors
    ///
    /// fails when a guard cannot be acquired or when the session ends without a
    /// value
    pub fn interact<W>(
        &self,
        widget: W,
        actions: impl IntoIterator<Item = ActionBinding>,
    ) -> Result<Value>
    where
        W: Widget + 'static,
    {
        self.run(widget, actions, None)
    }

    fn run<W>(
        &self,
        widget: W,
        actions: impl IntoIterator<Item = ActionBinding>,
        summary: Option<SummaryAnswer<'_>>,
    ) -> Result<Value>
    where
        W: Widget + 'static,
    {
        let mut guards = GuardStack(Vec::with_capacity(self.guards.len()));
        for factory in &self.guards {
            guards.0.push(factory()?);
        }
        let layered = ActionLayer::new(widget).with_actions(actions);
        let result = (self.runner)(Box::new(layered));
        if let (Ok(value), Some(answer), Some(sink)) = (&result, summary, &self.summary_sink)
            && let Some(line) = answer(value)
        {
            drop(sink(&line));
        }
        drop(guards);
        result
    }
}

#[derive(Clone, Copy)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "the prompt modules build summaries field by field"
)]
pub(crate) struct Summary<'a> {
    pub(crate) enabled: Option<bool>,
    pub(crate) answer:  SummaryAnswer<'a>,
}

impl<'a> Summary<'a> {
    pub(crate) const fn new(enabled: Option<bool>, answer: SummaryAnswer<'a>) -> Self {
        Self { enabled, answer }
    }
}

fn sanitised_line(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => skip_escape(&mut chars),
            '\u{9b}' => skip_csi(&mut chars),
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => skip_string(&mut chars),
            '\u{2028}' | '\u{2029}' => plain.push(' '),
            '\u{61c}'
            | '\u{200b}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}' => {},
            _ => plain.push(ch),
        }
    }
    screw::single_line(&plain).into_owned()
}

fn skip_escape(chars: &mut Peekable<Chars<'_>>) {
    match chars.next() {
        Some('[') => skip_csi(chars),
        Some(']' | 'P' | 'X' | '^' | '_') => skip_string(chars),
        Some(' '..='/') => {
            while chars.next_if(|ch| matches!(ch, ' '..='/')).is_some() {}
            chars.next_if(|ch| matches!(ch, '0'..='~'));
        },
        _ => {},
    }
}

fn skip_csi(chars: &mut Peekable<Chars<'_>>) {
    while chars.next_if(|ch| matches!(ch, ' '..='?')).is_some() {}
    chars.next_if(|ch| matches!(ch, '@'..='~'));
}

fn skip_string(chars: &mut Peekable<Chars<'_>>) {
    while let Some(ch) = chars.next() {
        match ch {
            '\x07' | '\u{9c}' => return,
            '\x1b' if chars.next_if_eq(&'\\').is_some() => return,
            _ => {},
        }
    }
}

fn summary_label(prompt: &str) -> String {
    prompt.trim().trim_end_matches(':').trim_end().to_owned()
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

/// a type-erased widget supplied to an advanced interaction runner
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

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        self.0.handle(event, cx)
    }

    fn current_value(&self) -> Option<Value> {
        self.0.current_value()
    }
}

pub(crate) fn scripted(
    scripts: impl IntoIterator<Item = impl IntoIterator<Item = Event>>,
) -> Interaction {
    let queued = scripts
        .into_iter()
        .map(|events| events.into_iter().collect::<Vec<_>>())
        .collect();
    let state = Rc::new(RefCell::new(Script {
        scripts:       queued,
        unused_events: 0,
        abandoned:     false,
    }));
    let abandon_state = Rc::clone(&state);
    let mut interaction = Interaction::from_runner(move |widget| {
        let events = state
            .borrow_mut()
            .scripts
            .pop_front()
            .ok_or_else(Error::input_ended)?;
        let mut remaining = events.into_iter();
        let result = replay_events(InteractionWidget::new(widget), &mut remaining);
        state.borrow_mut().unused_events += remaining.count();
        result
    });
    interaction.abandon = Some(Rc::new(move || abandon_state.borrow_mut().abandoned = true));
    interaction
}

struct Script {
    scripts:       VecDeque<Vec<Event>>,
    unused_events: usize,
    abandoned:     bool,
}

impl Drop for Script {
    fn drop(&mut self) {
        if self.abandoned || thread::panicking() {
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
    use bang_core::widgets::TextInput;

    use super::*;

    struct RecordedGuard {
        label:  &'static str,
        events: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Drop for RecordedGuard {
        fn drop(&mut self) {
            self.events.borrow_mut().push(self.label);
        }
    }

    #[test]
    fn summary_lines_drop_whole_escape_sequences_and_collapse_line_breaks() {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let sink_lines = Rc::clone(&lines);
        let interaction = Interaction::from_runner(|_widget| Ok(Value::from("done")))
            .with_summary_sink(Rc::new(move |line| {
                sink_lines.borrow_mut().push(line.to_owned());
                Ok(())
            }));
        let answer = |_: &Value| Some("red\x1b[31m\nblue\r\ngreen".to_owned());

        interaction
            .interact_named(
                Some("Pick\x1b]0;title\x07 one:"),
                TextInput::new("w"),
                [],
                Summary::new(None, &answer),
            )
            .unwrap();

        assert_eq!(*lines.borrow(), ["Pick one › red blue green"]);
    }

    #[test]
    fn summary_lines_drop_every_escape_family_and_hiding_format_characters() {
        for (raw, plain) in [
            ("a\x1b[31mb", "ab"),
            ("a\x1b[?25;1:2 qb", "ab"),
            ("a\x1b]0;t\x07b", "ab"),
            ("a\x1b]8;;http://x\x1b\\b\x1b]8;;\x1b\\c", "abc"),
            ("a\x1bPq#0\x1b\\b", "ab"),
            ("a\x1b(Bb", "ab"),
            ("a\x1bcb", "ab"),
            ("a\u{9b}31mb", "ab"),
            ("a\u{9d}0;t\u{9c}b", "ab"),
            ("a\u{202e}b\u{2066}c\u{2069}d", "abcd"),
            ("a\u{200b}b\u{feff}c\u{200e}d", "abcd"),
            ("a\u{2028}b\u{2029}c", "a b c"),
            (
                "a\u{1f469}\u{200d}\u{1f4bb}b",
                "a\u{1f469}\u{200d}\u{1f4bb}b",
            ),
        ] {
            assert_eq!(sanitised_line(raw), plain, "{raw:?}");
        }
    }

    #[test]
    #[should_panic(expected = "1 unused scripts and 0 unused events")]
    fn dropping_a_scripted_interaction_with_an_unstarted_script_panics() {
        drop(scripted([Vec::new(), Vec::new()]).interact(TextInput::new("w"), []));
    }

    #[test]
    #[should_panic(expected = "0 unused scripts and 1 unused events")]
    fn dropping_a_scripted_interaction_with_unread_events_panics() {
        let submit = Event::Key(bang_core::KeyEvent::new(bang_core::Key::Enter));
        let interaction = scripted([[submit.clone(), submit]]);
        interaction.interact(TextInput::new("w"), []).unwrap();
    }

    #[test]
    fn a_fully_consumed_scripted_interaction_drops_quietly() {
        let submit = Event::Key(bang_core::KeyEvent::new(bang_core::Key::Enter));
        scripted([[submit]])
            .interact(TextInput::new("w"), [])
            .unwrap();
    }

    #[test]
    fn guards_are_acquired_in_order_and_released_lifo() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let runner_events = Rc::clone(&events);
        let base = Interaction::from_runner(move |_widget| {
            runner_events.borrow_mut().push("run");
            Ok(Value::from("done"))
        });
        let first_events = Rc::clone(&events);
        let with_first = base.with_guard(move || {
            first_events.borrow_mut().push("acquire first");
            Ok(RecordedGuard {
                label:  "release first",
                events: Rc::clone(&first_events),
            })
        });
        let second_events = Rc::clone(&events);
        let interaction = with_first.with_guard(move || {
            second_events.borrow_mut().push("acquire second");
            Ok(RecordedGuard {
                label:  "release second",
                events: Rc::clone(&second_events),
            })
        });

        interaction.interact(TextInput::new("widget"), []).unwrap();

        assert_eq!(*events.borrow(), [
            "acquire first",
            "acquire second",
            "run",
            "release second",
            "release first",
        ]);
    }

    #[test]
    fn acquired_guards_unwind_lifo_when_later_acquisition_fails() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let first_events = Rc::clone(&events);
        let with_first = Interaction::disabled().with_guard(move || {
            first_events.borrow_mut().push("acquire first");
            Ok(RecordedGuard {
                label:  "release first",
                events: Rc::clone(&first_events),
            })
        });
        let second_events = Rc::clone(&events);
        let interaction = with_first.with_guard(move || -> Result<RecordedGuard> {
            second_events.borrow_mut().push("acquire second");
            Err(Error::interaction_unavailable())
        });

        let error = interaction
            .interact(TextInput::new("widget"), [])
            .unwrap_err();

        assert_eq!(error.kind(), crate::ErrorKind::InteractionUnavailable);
        assert_eq!(*events.borrow(), [
            "acquire first",
            "acquire second",
            "release first"
        ]);
    }
}
