// SPDX-License-Identifier: EUPL-1.2

use std::io::{
    self,
    Read,
};

use bang_core::{
    Event,
    Key,
    Modifiers,
    Reaction,
    Session,
    SessionStatus,
    Value,
    View,
    Widget,
};

use crate::{
    Decoder,
    SignalGuard,
    TerminalSize,
    terminal_size,
};

#[derive(Clone, Debug, PartialEq)]
pub enum RunOutcome {
    Submitted(Value),
    Cancelled,
    InputEnded,
    Signalled(i32),
}

pub trait SessionRenderer {
    fn render(&mut self, view: &View) -> io::Result<()>;

    fn resize(&mut self, _size: TerminalSize) -> io::Result<()> {
        Ok(())
    }
}

pub fn drive_blocking_session(
    widget: impl Widget + 'static,
    mut input: impl Read,
    renderer: &mut impl SessionRenderer,
) -> io::Result<RunOutcome> {
    drive(
        widget,
        &mut input,
        renderer,
        ZeroRead::End,
        TerminalSizeTracker::new(terminal_size()),
        &mut NoSignals,
    )
}

pub fn drive_tty_session(
    widget: impl Widget + 'static,
    mut input: impl Read,
    renderer: &mut impl SessionRenderer,
) -> io::Result<RunOutcome> {
    drive_tty_session_with_signal_source(widget, &mut input, renderer, &mut NoSignals)
}

pub fn drive_tty_session_with_signals(
    widget: impl Widget + 'static,
    mut input: impl Read,
    renderer: &mut impl SessionRenderer,
    signals: &mut SignalGuard,
) -> io::Result<RunOutcome> {
    drive_tty_session_with_signal_source(widget, &mut input, renderer, signals)
}

fn drive_tty_session_with_signal_source(
    widget: impl Widget + 'static,
    input: &mut impl Read,
    renderer: &mut impl SessionRenderer,
    signals: &mut impl SignalSource,
) -> io::Result<RunOutcome> {
    drive(
        widget,
        input,
        renderer,
        ZeroRead::Timeout,
        TerminalSizeTracker::new(terminal_size()),
        signals,
    )
}

fn drive(
    widget: impl Widget + 'static,
    input: &mut impl Read,
    renderer: &mut impl SessionRenderer,
    zero_read: ZeroRead,
    mut size: TerminalSizeTracker,
    signals: &mut impl SignalSource,
) -> io::Result<RunOutcome> {
    let mut session = Session::new(widget);
    let mut decoder = Decoder::new();
    let mut buffer = [0; 64];

    if let Some(event) = size.initial_event() {
        let _reaction = session.handle(event);
    }
    if let Some(size) = size.current() {
        renderer.resize(size)?;
    }
    render_if_dirty(&mut session, renderer)?;

    loop {
        if let Some(signal) = signals.poll_signal()? {
            return Ok(RunOutcome::Signalled(signal));
        }

        let read = match input.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                if let Some(signal) = signals.poll_signal()? {
                    return Ok(RunOutcome::Signalled(signal));
                }
                continue;
            },
            Err(error) => return Err(error),
        };
        if read == 0 {
            for event in decoder.flush() {
                if let Some(outcome) = handle_event(&mut session, event, renderer)? {
                    return Ok(outcome);
                }
            }
            match zero_read {
                ZeroRead::End => return Ok(outcome_from_status(session.status())),
                ZeroRead::Timeout => {
                    if let Some(signal) = signals.poll_signal()? {
                        return Ok(RunOutcome::Signalled(signal));
                    }
                    if let Some(event) = size.poll_event()
                        && let Some(outcome) = handle_event(&mut session, event, renderer)?
                    {
                        return Ok(outcome);
                    }
                    continue;
                },
            }
        }

        for event in decoder.feed(&buffer[..read]) {
            if let Some(outcome) = handle_event(&mut session, event, renderer)? {
                return Ok(outcome);
            }
        }
    }
}

trait SignalSource {
    fn poll_signal(&mut self) -> io::Result<Option<i32>>;
}

struct NoSignals;

impl SignalSource for NoSignals {
    fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        Ok(None)
    }
}

impl SignalSource for SignalGuard {
    fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        self.poll_signal()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ZeroRead {
    End,
    Timeout,
}

struct TerminalSizeTracker {
    last: Option<TerminalSize>,
}

impl TerminalSizeTracker {
    const fn new(initial: Option<TerminalSize>) -> Self {
        Self { last: initial }
    }

    fn initial_event(&self) -> Option<Event> {
        self.last.map(resize_event)
    }

    const fn current(&self) -> Option<TerminalSize> {
        self.last
    }

    fn poll_event(&mut self) -> Option<Event> {
        let next = terminal_size();
        if next == self.last {
            return None;
        }
        self.last = next;
        next.map(resize_event)
    }
}

const fn resize_event(size: TerminalSize) -> Event {
    Event::Resize {
        cols: size.cols,
        rows: size.rows,
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
        Reaction::Ignored | Reaction::Changed | Reaction::Focus(_) => {
            match session.status() {
                SessionStatus::Submitted(value) => Some(RunOutcome::Submitted(value.clone())),
                SessionStatus::Cancelled => Some(RunOutcome::Cancelled),
                SessionStatus::Running => None,
            }
        },
    })
}

fn render_if_dirty(session: &mut Session, renderer: &mut impl SessionRenderer) -> io::Result<()> {
    if session.is_dirty() {
        renderer.render(&session.view())?;
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
