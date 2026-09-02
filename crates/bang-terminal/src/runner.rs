// SPDX-License-Identifier: EUPL-1.2

use std::io::{self, Read};
use std::os::fd::AsFd;

use bang_core::{
    Event, Key, Modifiers, Presentation, Reaction, Session, SessionStatus, Value, Widget,
    adapter::View,
};

use crate::{
    Clock, ProcessTerminalSize, SignalGuard, SignalSource, TerminalEvents, TerminalPoll,
    TerminalSize, TerminalSizeSource,
};

#[derive(Clone, Debug, PartialEq)]
pub enum RunOutcome {
    Submitted(Value),
    Cancelled,
    InputEnded,
    Signalled(i32),
}

pub trait SessionRenderer {
    fn render(&mut self, view: &View) -> io::Result<Presentation>;

    fn resize(&mut self, _size: TerminalSize) -> io::Result<()> {
        Ok(())
    }
}

pub fn drive_blocking_session(
    widget: impl Widget + 'static,
    input: impl Read + Send + 'static,
    renderer: &mut impl SessionRenderer,
) -> io::Result<RunOutcome> {
    let mut events = TerminalEvents::blocking(input).with_resize_source(ProcessTerminalSize);
    drive(widget, &mut events, renderer)
}

pub fn drive_tty_session(
    widget: impl Widget + 'static,
    mut input: impl Read + AsFd,
    renderer: &mut impl SessionRenderer,
) -> io::Result<RunOutcome> {
    let mut events = TerminalEvents::tty(&mut input)?.with_resize_source(ProcessTerminalSize);
    drive(widget, &mut events, renderer)
}

pub fn drive_tty_session_with_signals(
    widget: impl Widget + 'static,
    mut input: impl Read + AsFd,
    renderer: &mut impl SessionRenderer,
    signals: &mut SignalGuard,
) -> io::Result<RunOutcome> {
    let mut events = TerminalEvents::tty(&mut input)?
        .with_signals(signals)
        .with_resize_source(ProcessTerminalSize);
    drive(widget, &mut events, renderer)
}

fn drive<R, S, Z, C>(
    widget: impl Widget + 'static,
    events: &mut TerminalEvents<R, S, Z, C>,
    renderer: &mut impl SessionRenderer,
) -> io::Result<RunOutcome>
where
    R: Read,
    S: SignalSource,
    Z: TerminalSizeSource,
    C: Clock,
{
    let mut session = Session::new(widget);
    if let Some(size) = events.initial_terminal_size()? {
        renderer.resize(size)?;
        let _reaction = session.handle(Event::Resize {
            cols: size.cols,
            rows: size.rows,
        });
    }
    render_if_dirty(&mut session, renderer)?;

    loop {
        match events.next_event()? {
            TerminalPoll::Event(event) => {
                if let Some(outcome) = handle_event(&mut session, event, renderer)? {
                    return Ok(outcome);
                }
            },
            TerminalPoll::Signal(signal) => return Ok(RunOutcome::Signalled(signal)),
            TerminalPoll::End => return Ok(outcome_from_status(session.status())),
        }
    }
}

fn handle_event(
    session: &mut Session,
    event: Event,
    renderer: &mut impl SessionRenderer,
) -> io::Result<Option<RunOutcome>> {
    if is_control_char(&event, 'c') {
        return Ok(Some(RunOutcome::Cancelled));
    }
    if is_control_char(&event, 'd') {
        return Ok(Some(RunOutcome::InputEnded));
    }

    if let Event::Resize { cols, rows } = &event {
        renderer.resize(TerminalSize {
            cols: *cols,
            rows: *rows,
        })?;
    }
    let reaction = session.handle(event);
    render_if_dirty(session, renderer)?;

    Ok(match reaction {
        Reaction::Submit(value) => Some(RunOutcome::Submitted(value)),
        Reaction::Cancel => Some(RunOutcome::Cancelled),
        Reaction::Ignored | Reaction::Changed | Reaction::Focus(_) => match session.status() {
            SessionStatus::Submitted(value) => Some(RunOutcome::Submitted(value.clone())),
            SessionStatus::Cancelled => Some(RunOutcome::Cancelled),
            SessionStatus::Running => None,
        },
    })
}

fn render_if_dirty(session: &mut Session, renderer: &mut impl SessionRenderer) -> io::Result<()> {
    if session.is_dirty() {
        let presentation = renderer.render(&session.view())?;
        session.set_presentation(presentation);
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

fn outcome_from_status(status: &SessionStatus) -> RunOutcome {
    match status {
        SessionStatus::Submitted(value) => RunOutcome::Submitted(value.clone()),
        SessionStatus::Cancelled => RunOutcome::Cancelled,
        SessionStatus::Running => RunOutcome::InputEnded,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, io::Cursor};

    use bang_core::{
        ListPresentation, Presentation, Role, Span, ViewContext, ViewId, WidgetId,
        adapter::plain_snapshot, widgets::TextInput,
    };

    use super::*;
    use crate::SystemClock;

    #[derive(Default)]
    struct FakeRenderer {
        views: Vec<String>,
        resizes: Vec<TerminalSize>,
        viewport: Option<TerminalSize>,
        viewports_at_render: Vec<Option<TerminalSize>>,
        presentation: Presentation,
        fail_render_at: Option<usize>,
    }

    impl SessionRenderer for FakeRenderer {
        fn render(&mut self, view: &View) -> io::Result<Presentation> {
            if self.fail_render_at == Some(self.views.len()) {
                return Err(io::Error::other("scripted renderer failure"));
            }
            self.views.push(plain_snapshot(view));
            self.viewports_at_render.push(self.viewport);
            Ok(self.presentation.clone())
        }

        fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
            self.resizes.push(size);
            self.viewport = Some(size);
            Ok(())
        }
    }

    struct RepeatedSize(TerminalSize);

    impl TerminalSizeSource for RepeatedSize {
        fn terminal_size(&mut self) -> io::Result<Option<TerminalSize>> {
            Ok(Some(self.0))
        }
    }

    struct ScriptedSignals(VecDeque<Option<i32>>);

    impl SignalSource for ScriptedSignals {
        fn poll_signal(&mut self) -> io::Result<Option<i32>> {
            Ok(self.0.pop_front().flatten())
        }
    }

    #[test]
    fn scripted_input_resize_and_submission_share_one_driver() {
        let size = TerminalSize { cols: 80, rows: 24 };
        let mut events =
            TerminalEvents::blocking(Cursor::new(b"ab\r")).with_resize_source(RepeatedSize(size));
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
        assert_eq!(
            renderer.views,
            ["name: ", "name: a", "name: ab", "name: ab"]
        );
    }

    #[test]
    fn renderer_feedback_is_available_to_the_next_event() {
        struct FeedbackWidget;

        impl Widget for FeedbackWidget {
            fn id(&self) -> WidgetId {
                WidgetId::borrowed("feedback")
            }

            fn handle(&mut self, _event: Event, cx: &mut bang_core::Context) -> Reaction {
                let visible = cx
                    .list_presentation(&ViewId::borrowed("choices"))
                    .map(|list| list.visible.clone());
                Reaction::Submit(Value::from(visible == Some(1..3)))
            }

            fn view(&self, _cx: &ViewContext) -> View {
                View::Text(vec![Span::new("feedback", Role::Normal)])
            }
        }

        let mut renderer = FakeRenderer {
            presentation: Presentation {
                lists: vec![ListPresentation {
                    id: ViewId::borrowed("choices"),
                    visible: 1..3,
                    fully_visible: 1..3,
                    page_up: Some(0),
                    page_down: Some(3),
                }],
            },
            ..FakeRenderer::default()
        };
        let mut events = TerminalEvents::blocking(Cursor::new(b"x"));

        assert_eq!(
            drive(FeedbackWidget, &mut events, &mut renderer).unwrap(),
            RunOutcome::Submitted(Value::from(true)),
        );
        assert_eq!(renderer.views, ["feedback", "feedback"]);
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

        let signals = ScriptedSignals(VecDeque::from([Some(15)]));
        let mut signal_events = TerminalEvents::blocking(Cursor::new(b"ignored"))
            .with_signals(signals)
            .with_resize_source(crate::NoTerminalSize)
            .with_clock(SystemClock);
        assert_eq!(
            drive(
                TextInput::new("value"),
                &mut signal_events,
                &mut FakeRenderer::default(),
            )
            .unwrap(),
            RunOutcome::Signalled(15),
        );
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
        assert_eq!(renderer.views, [""]);
    }
}
