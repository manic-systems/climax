// SPDX-License-Identifier: EUPL-1.2

use core::time::Duration;
use std::{
    io::{
        self,
        Read,
    },
    os::fd::{
        AsFd,
        OwnedFd,
    },
    time::Instant,
};

use bang_core::{
    Event,
    Key,
    Modifiers,
    Session,
    SessionReaction,
    SessionStatus,
    Value,
    Widget,
};
use bang_terminal::{
    Signal,
    SignalPoller,
    TerminalEvents,
    TerminalPoll,
    resize_event,
};
use screw::{
    TickInterest,
    Viewport,
    Widget as _,
};

const FRAME_INTERVAL: Duration = Duration::from_millis(66);

/// how a driven session ended
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RunOutcome {
    /// the widget produced a value
    Submitted(Value),
    /// the widget or an unclaimed ctrl-c cancelled the session
    Cancelled,
    /// input ended or an unclaimed ctrl-d closed it before a value was produced
    InputEnded,
    /// a signal arrived
    Signalled(Signal),
}

/// draws the frames a driven session produces
pub(crate) trait SessionRenderer {
    /// draw the session as it is now
    fn render(&mut self, session: &Session) -> io::Result<()>;

    /// the terminal changed size. called before the session is drawn again
    fn resize(&mut self, _size: Viewport) -> io::Result<()> {
        Ok(())
    }
}

/// optional inputs to [`drive_tty_session`], by default no signals and no
/// resizes
#[derive(Debug, Default)]
pub(crate) struct SessionOptions {
    signals: Option<SignalPoller>,
    resize:  Option<OwnedFd>,
}

impl SessionOptions {
    /// no signal handling and no resize events
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            signals: None,
            resize:  None,
        }
    }

    /// stop the session with [`RunOutcome::Signalled`] when `signals` yields
    /// one
    #[must_use]
    pub(crate) const fn signals(mut self, signals: SignalPoller) -> Self {
        self.signals = Some(signals);
        self
    }

    /// report resizes of the terminal behind `terminal`, and size the first
    /// frame from it
    #[must_use]
    pub(crate) fn resize_from(mut self, terminal: OwnedFd) -> Self {
        self.resize = Some(terminal);
        self
    }
}

/// run `widget` as an interactive session on `input`, drawing through
/// `renderer`
///
/// the caller owns terminal setup, so raw mode and any screen modes must
/// already be active. `input` is polled for bytes with an escape deadline, and
/// ctrl-c and ctrl-d end the session only when the widget ignores them
pub(crate) fn drive_tty_session(
    widget: impl Widget + 'static,
    input: impl Read + AsFd,
    renderer: &mut impl SessionRenderer,
    options: SessionOptions,
) -> io::Result<RunOutcome> {
    let mut events = TerminalEvents::pollable(input)?;
    if let Some(signals) = options.signals {
        events = events.signals(signals);
    }
    if let Some(terminal) = options.resize {
        events = events.resize_from(terminal);
    }
    drive(widget, &mut events, renderer)
}

fn drive<R: Read>(
    widget: impl Widget + 'static,
    events: &mut TerminalEvents<R>,
    renderer: &mut impl SessionRenderer,
) -> io::Result<RunOutcome> {
    let mut session = Session::new(widget);
    if let Some(size) = events.initial_terminal_size()
        && let Some(outcome) = handle_event(&mut session, resize_event(size), renderer)?
    {
        return Ok(outcome);
    }
    render_if_dirty(&mut session, renderer)?;

    let mut last_tick = Instant::now();
    loop {
        let wait = tick_interval(session.tick_interest())
            .map(|interval| interval.saturating_sub(last_tick.elapsed()));
        let Some(polled) = events.next_event_within(wait)? else {
            last_tick = Instant::now();
            if let Some(outcome) = handle_event(&mut session, Event::Tick, renderer)? {
                return Ok(outcome);
            }
            continue;
        };
        match polled {
            TerminalPoll::Event(event) => {
                if let Some(outcome) = handle_event(&mut session, event, renderer)? {
                    return Ok(outcome);
                }
            },
            TerminalPoll::Signal(signal) => return Ok(RunOutcome::Signalled(signal)),
            TerminalPoll::End | _ => return Ok(outcome_from_status(session.status())),
        }
    }
}

fn tick_interval(interest: TickInterest) -> Option<Duration> {
    match interest {
        TickInterest::Never => None,
        TickInterest::EveryFrame => Some(FRAME_INTERVAL),
        TickInterest::Every(interval) => Some(interval.max(FRAME_INTERVAL)),
    }
}

/// live session event dispatch
///
/// ctrl-c and ctrl-d are offered to the widget first, and an ignored ctrl-c
/// cancels while an ignored ctrl-d ends input
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "the session enums are non-exhaustive"
)]
fn handle_event(
    session: &mut Session,
    event: Event,
    renderer: &mut impl SessionRenderer,
) -> io::Result<Option<RunOutcome>> {
    let unhandled = unhandled_outcome(&event);

    if let Event::Resize { cols, rows } = &event {
        renderer.resize(Viewport {
            columns: usize::from(*cols),
            rows:    usize::from(*rows),
        })?;
    }
    let reaction = session.handle(event);
    render_if_dirty(session, renderer)?;

    let ignored = matches!(reaction, SessionReaction::Ignored);
    Ok(match reaction {
        SessionReaction::Submit(value) => Some(RunOutcome::Submitted(value)),
        SessionReaction::Cancel => Some(RunOutcome::Cancelled),
        _ => {
            match session.status() {
                SessionStatus::Submitted(value) => Some(RunOutcome::Submitted(value.clone())),
                SessionStatus::Cancelled => Some(RunOutcome::Cancelled),
                SessionStatus::Running if ignored => unhandled,
                _ => None,
            }
        },
    })
}

pub(crate) fn unhandled_outcome(event: &Event) -> Option<RunOutcome> {
    if is_control_char(event, 'c') {
        return Some(RunOutcome::Cancelled);
    }
    if is_control_char(event, 'd') {
        return Some(RunOutcome::InputEnded);
    }
    None
}

fn render_if_dirty(session: &mut Session, renderer: &mut impl SessionRenderer) -> io::Result<()> {
    if session.is_dirty() {
        renderer.render(session)?;
        session.clear_dirty();
    }
    Ok(())
}

fn is_control_char(event: &Event, value: char) -> bool {
    matches!(
        event,
        Event::Key(key)
            if key.key == Key::Char(value) && key.modifiers.contains(Modifiers::CONTROL)
    )
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "the session enums are non-exhaustive"
)]
fn outcome_from_status(status: &SessionStatus) -> RunOutcome {
    match status {
        SessionStatus::Submitted(value) => RunOutcome::Submitted(value.clone()),
        SessionStatus::Cancelled => RunOutcome::Cancelled,
        _ => RunOutcome::InputEnded,
    }
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;
    use std::{
        fs::File,
        io::Cursor,
        sync::{
            Mutex,
            mpsc,
        },
        thread,
    };

    use bang_core::{
        Reaction,
        WidgetId,
        widgets::TextInput,
    };
    use bang_terminal::{
        SignalGuard,
        testing::{
            pty,
            serialise_signals,
        },
    };
    use screw::{
        RenderCtx,
        Surface,
    };

    use super::*;

    #[derive(Default)]
    struct FakeRenderer {
        frames:              Vec<String>,
        resizes:             Vec<Viewport>,
        viewport:            Option<Viewport>,
        viewports_at_render: Vec<Option<Viewport>>,
        fail_render_at:      Option<usize>,
    }

    impl SessionRenderer for FakeRenderer {
        fn render(&mut self, session: &Session) -> io::Result<()> {
            if self.fail_render_at == Some(self.frames.len()) {
                return Err(io::Error::other("scripted renderer failure"));
            }
            self.frames.push(screw::render_plain(session));
            self.viewports_at_render.push(self.viewport);
            Ok(())
        }

        fn resize(&mut self, size: Viewport) -> io::Result<()> {
            self.resizes.push(size);
            self.viewport = Some(size);
            Ok(())
        }
    }

    #[test]
    fn scripted_input_resize_and_submission_share_one_driver() {
        let size = Viewport {
            columns: 80,
            rows:    24,
        };
        let (_master, slave) = pty(80, 24).unwrap();
        let mut events = TerminalEvents::blocking(Cursor::new(b"ab\r")).resize_from(slave);
        let mut renderer = FakeRenderer::default();

        let outcome = drive(
            TextInput::new("name").with_prompt("name: "),
            &mut events,
            &mut renderer,
        )
        .unwrap();

        assert_eq!(outcome, RunOutcome::Submitted(Value::from("ab")));
        assert_eq!(renderer.resizes, [size]);
        assert!(
            renderer
                .viewports_at_render
                .iter()
                .all(|viewport| *viewport == Some(size))
        );
        assert_eq!(renderer.frames, [
            "name: ", "name: a", "name: ab", "name: ab"
        ]);
    }

    #[test]
    fn eof_control_c_and_signals_remain_distinct_outcomes() {
        let mut eof_events = TerminalEvents::blocking(Cursor::new(b"x"));
        assert_eq!(
            drive(
                TextInput::new("value"),
                &mut eof_events,
                &mut FakeRenderer::default(),
            )
            .unwrap(),
            RunOutcome::InputEnded,
        );

        let mut cancel_events = TerminalEvents::blocking(Cursor::new([3_u8]));
        assert_eq!(
            drive(
                TextInput::new("value"),
                &mut cancel_events,
                &mut FakeRenderer::default(),
            )
            .unwrap(),
            RunOutcome::Cancelled,
        );

        let _serial = serialise_signals();
        let guard = SignalGuard::install_terminal_handlers().unwrap();
        let mut signal_events =
            TerminalEvents::blocking(Cursor::new(b"ignored")).signals(guard.poller());
        // SAFETY: raising a signal the guard has a handler for
        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        assert_eq!(
            drive(
                TextInput::new("value"),
                &mut signal_events,
                &mut FakeRenderer::default(),
            )
            .unwrap(),
            RunOutcome::Signalled(Signal::TERM),
        );
        guard.restore().unwrap();
    }

    #[test]
    fn a_widget_that_claims_control_c_keeps_the_session_running() {
        struct ClaimsControlC(usize);

        impl screw::Widget for ClaimsControlC {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                out.write("claims", screw::Style::default());
            }
        }

        impl Widget for ClaimsControlC {
            fn id(&self) -> WidgetId {
                WidgetId::borrowed("claims")
            }

            fn handle(&mut self, event: Event, _cx: &mut bang_core::Context) -> Reaction {
                match event {
                    Event::Key(key) if key.key == Key::Char('c') => {
                        self.0 += 1;
                        if self.0 == 2 {
                            return Reaction::Submit(Value::from("claimed twice"));
                        }
                        Reaction::Changed
                    },
                    Event::Key(_)
                    | Event::Paste(_)
                    | Event::UnknownEscape(_)
                    | Event::Resize { .. }
                    | Event::Tick
                    | _ => Reaction::Ignored,
                }
            }
        }

        let mut events = TerminalEvents::blocking(Cursor::new([3_u8, 3_u8]));

        assert_eq!(
            drive(ClaimsControlC(0), &mut events, &mut FakeRenderer::default()).unwrap(),
            RunOutcome::Submitted(Value::from("claimed twice")),
        );
    }

    #[test]
    fn reserved_keys_reach_the_widget_before_the_runner_acts_on_them() {
        #[derive(Clone, Default)]
        struct SeenKeys(Arc<Mutex<Vec<char>>>);

        impl screw::Widget for SeenKeys {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                out.write("seen", screw::Style::default());
            }
        }

        impl Widget for SeenKeys {
            fn id(&self) -> WidgetId {
                WidgetId::borrowed("seen")
            }

            fn handle(&mut self, event: Event, _cx: &mut bang_core::Context) -> Reaction {
                if let Event::Key(key) = event
                    && let Key::Char(value) = key.key
                {
                    self.0.lock().unwrap().push(value);
                }
                Reaction::Ignored
            }
        }

        for (byte, expected, outcome) in [
            (3_u8, 'c', RunOutcome::Cancelled),
            (4_u8, 'd', RunOutcome::InputEnded),
        ] {
            let widget = SeenKeys::default();
            let seen = Arc::clone(&widget.0);
            let mut events = TerminalEvents::blocking(Cursor::new([byte]));

            assert_eq!(
                drive(widget, &mut events, &mut FakeRenderer::default()).unwrap(),
                outcome,
            );
            assert_eq!(*seen.lock().unwrap(), vec![expected]);
        }
    }

    #[test]
    fn a_widget_that_submits_on_the_initial_resize_ends_the_session_before_input() {
        struct SubmitsOnResize;

        impl screw::Widget for SubmitsOnResize {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                out.write("resize", screw::Style::default());
            }
        }

        impl Widget for SubmitsOnResize {
            fn id(&self) -> WidgetId {
                WidgetId::borrowed("resize")
            }

            fn handle(&mut self, event: Event, _cx: &mut bang_core::Context) -> Reaction {
                match event {
                    Event::Resize { .. } => Reaction::Submit(Value::from("sized")),
                    Event::Key(_) | Event::Paste(_) | Event::UnknownEscape(_) | Event::Tick | _ => {
                        Reaction::Ignored
                    },
                }
            }
        }

        let (master, slave) = pty(80, 24).unwrap();
        let input = File::from(slave.try_clone().unwrap());
        let (done, outcome) = mpsc::channel();
        thread::spawn(move || {
            let options = SessionOptions::new().resize_from(slave);
            let result = drive_tty_session(
                SubmitsOnResize,
                input,
                &mut FakeRenderer::default(),
                options,
            );
            drop(done.send(result.unwrap()));
        });

        assert_eq!(
            outcome.recv_timeout(Duration::from_secs(5)).ok(),
            Some(RunOutcome::Submitted(Value::from("sized"))),
        );
        drop(master);
    }

    #[test]
    fn renderer_failures_abort_before_more_input_is_consumed() {
        let mut renderer = FakeRenderer {
            fail_render_at: Some(1),
            ..FakeRenderer::default()
        };
        let mut events = TerminalEvents::blocking(Cursor::new(b"ab\r"));

        let error = drive(TextInput::new("value"), &mut events, &mut renderer).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(renderer.frames, [""]);
    }
}
