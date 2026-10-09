// SPDX-License-Identifier: EUPL-1.2

use core::{
    error,
    fmt,
};
use std::{
    env,
    fs::File,
    io::{
        self,
        IsTerminal as _,
        Write,
    },
    os::fd::{
        AsFd,
        OwnedFd,
    },
};

use bang_core::{
    Session,
    Value,
    Widget as BangWidget,
};
use bang_terminal::{
    CleanupFailure,
    CleanupFailures,
    CleanupStage,
    Signal,
};
use screw::{
    CursorVisibility,
    RenderStats,
    Renderer,
    Viewport,
};

use crate::session::{
    RunOutcome,
    SessionOptions,
    SessionRenderer,
    drive_tty_session,
};

/// a [`SessionRenderer`] over a `screw` renderer that follows terminal resizes
struct ScrewSessionRenderer<'a, W>
where
    W: Write,
{
    renderer: Renderer<&'a mut W>,
}

impl<'a, W> ScrewSessionRenderer<'a, W>
where
    W: Write,
{
    fn with_initial_size(writer: &'a mut W, size: Option<Viewport>) -> Self {
        let mut renderer = Renderer::new(writer).cursor_visibility(CursorVisibility::FromSurface);
        if let Some(measured) = size {
            renderer = renderer.width(measured.columns);
        }
        Self { renderer }
    }

    fn clear(&mut self) -> io::Result<RenderStats> {
        self.renderer.clear()
    }
}

impl<W> SessionRenderer for ScrewSessionRenderer<'_, W>
where
    W: Write,
{
    fn render(&mut self, session: &Session) -> io::Result<()> {
        self.renderer.draw(session).map(drop)
    }

    fn resize(&mut self, size: Viewport) -> io::Result<()> {
        self.renderer.resize_viewport(size.columns, size.rows);
        Ok(())
    }
}

/// why a live session did not return a value
#[derive(Debug)]
#[non_exhaustive]
pub enum LiveSessionError {
    /// input or output is not an interactive terminal, or `TERM` is `dumb`
    Unavailable,
    /// raw mode could not be enabled
    RawMode(io::Error),
    /// terminal signal handlers could not be installed
    Signals(io::Error),
    /// reading input or drawing failed
    TerminalIo(io::Error),
    /// the widget cancelled, or ctrl-c went unclaimed
    Cancelled,
    /// input ended before the widget submitted
    InputEnded,
    /// a terminal signal arrived, the terminal and previous handlers are
    /// restored and the process carries on
    ///
    /// a signal landing during wind down wins over a value just submitted
    Signalled(Signal),
    /// restoring the terminal failed, possibly after another error
    Cleanup {
        /// the error the session ended with before cleanup, if any
        primary:  Option<Box<Self>>,
        /// every cleanup step that failed
        failures: CleanupFailures,
    },
}

impl LiveSessionError {
    /// the error the session itself ended with, looking through a cleanup
    /// wrapper, `None` when only cleanup failed
    #[must_use]
    pub fn primary(&self) -> Option<&Self> {
        match self {
            Self::Cleanup { primary, .. } => primary.as_deref(),
            Self::Unavailable
            | Self::RawMode(_)
            | Self::Signals(_)
            | Self::TerminalIo(_)
            | Self::Cancelled
            | Self::InputEnded
            | Self::Signalled(_) => Some(self),
        }
    }

    /// every teardown failure, empty unless this is [`Self::Cleanup`]
    #[must_use]
    pub fn cleanup_failures(&self) -> &[CleanupFailure] {
        match self {
            Self::Cleanup { failures, .. } => failures.failures(),
            Self::Unavailable
            | Self::RawMode(_)
            | Self::Signals(_)
            | Self::TerminalIo(_)
            | Self::Cancelled
            | Self::InputEnded
            | Self::Signalled(_) => &[],
        }
    }
}

impl fmt::Display for LiveSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("interactive terminal input is unavailable"),
            Self::RawMode(error) => {
                if matches!(
                    error.kind(),
                    io::ErrorKind::Unsupported | io::ErrorKind::NotConnected
                ) {
                    write!(f, "{error}")
                } else {
                    write!(f, "failed to enable terminal raw mode: {error}")
                }
            },
            Self::Signals(error) => {
                write!(f, "failed to install terminal signal handlers: {error}")
            },
            Self::TerminalIo(error) => write!(f, "terminal I/O failed: {error}"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::InputEnded => f.write_str("input ended before submit"),
            Self::Signalled(signal) => write!(f, "interrupted by signal {signal}"),
            Self::Cleanup { primary, failures } => {
                if let Some(inner) = primary {
                    write!(f, "{inner}; ")?;
                }
                write!(f, "terminal cleanup failed: {failures}")
            },
        }
    }
}

impl error::Error for LiveSessionError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::RawMode(error) | Self::Signals(error) | Self::TerminalIo(error) => Some(error),
            Self::Cleanup { primary, failures } => {
                primary
                    .as_deref()
                    .map(|error| -> &(dyn error::Error + 'static) { error })
                    .or(Some(failures))
            },
            Self::Unavailable | Self::Cancelled | Self::InputEnded | Self::Signalled(_) => None,
        }
    }
}

/// run a session on stdin and stderr, drawing with `screw`
///
/// fails with [`LiveSessionError::Unavailable`] unless both are terminals and
/// `TERM` is not `dumb`
pub(crate) fn run_live_session(
    widget: impl BangWidget + 'static,
) -> Result<Value, LiveSessionError> {
    if !io::stdin().is_terminal()
        || !io::stderr().is_terminal()
        || env::var_os("TERM").is_some_and(|term| term == "dumb")
    {
        return Err(LiveSessionError::Unavailable);
    }
    run_live_session_forced(widget)
}

/// run a session without first applying terminal capability policy
pub(crate) fn run_live_session_forced(
    widget: impl BangWidget + 'static,
) -> Result<Value, LiveSessionError> {
    let stdin = io::stdin().lock();
    let stderr = io::stderr();
    let resize = stderr
        .as_fd()
        .try_clone_to_owned()
        .map_err(LiveSessionError::TerminalIo)?;
    let mut locked = stderr.lock();
    run_live_session_forced_with(widget, stdin, &mut locked, resize)
}

/// run a session on a caller-owned terminal handle such as `/dev/tty`, checking
/// it is a terminal and `TERM` is not `dumb` first
///
/// raw mode, input, drawing and size detection all use `handle`, not the
/// process's own stdin and stderr
pub(crate) fn run_live_session_on(
    widget: impl BangWidget + 'static,
    handle: impl Into<OwnedFd>,
) -> Result<Value, LiveSessionError> {
    let owned = handle.into();
    if !owned.is_terminal() || env::var_os("TERM").is_some_and(|term| term == "dumb") {
        return Err(LiveSessionError::Unavailable);
    }
    run_live_session_forced_on(widget, owned)
}

/// run a session on a caller-owned terminal handle without checking terminal
/// capability
pub(crate) fn run_live_session_forced_on(
    widget: impl BangWidget + 'static,
    handle: impl Into<OwnedFd>,
) -> Result<Value, LiveSessionError> {
    let file = File::from(handle.into());
    let mut writer = &file;
    let resize = file
        .as_fd()
        .try_clone_to_owned()
        .map_err(LiveSessionError::TerminalIo)?;
    run_live_session_forced_with(widget, &file, &mut writer, resize)
}

fn run_live_session_forced_with<I, O>(
    widget: impl BangWidget + 'static,
    input: I,
    output: &mut O,
    resize: OwnedFd,
) -> Result<Value, LiveSessionError>
where
    I: io::Read + AsFd,
    O: Write,
{
    let signals = bang_terminal::SignalGuard::install_terminal_handlers()
        .map_err(LiveSessionError::Signals)?;
    let terminal = match bang_terminal::TerminalModeGuard::activate(
        &input,
        bang_terminal::RawModeOptions::blocking(),
    ) {
        Ok(terminal) => terminal,
        Err(error) => {
            let mut failures = Vec::new();
            collect_cleanup(&mut failures, signals.restore());
            return Err(with_cleanup(LiveSessionError::RawMode(error), failures));
        },
    };
    let mut screen =
        match bang_terminal::ScreenGuard::enter(output, bang_terminal::ScreenOptions::inline()) {
            Ok(screen) => screen,
            Err(error) => {
                let mut failures = Vec::new();
                collect_cleanup(&mut failures, terminal.restore());
                collect_cleanup(&mut failures, signals.restore());
                return Err(with_cleanup(LiveSessionError::TerminalIo(error), failures));
            },
        };
    let initial_size = Viewport::of(&resize).ok();
    let mut renderer = ScrewSessionRenderer::with_initial_size(screen.writer(), initial_size);
    let outcome = drive_tty_session(
        widget,
        input,
        &mut renderer,
        SessionOptions::new()
            .signals(signals.poller())
            .resize_from(resize),
    );
    let clear = renderer.clear();
    drop(renderer);
    let screen_cleanup = screen.leave();
    let raw_cleanup = terminal.restore();
    let (signal_cleanup, late_signal) = signals.restore_polling();

    let settled = match (outcome, late_signal) {
        (Ok(RunOutcome::Signalled(signal)), _) | (Ok(_), Some(signal)) => {
            Ok(RunOutcome::Signalled(signal))
        },
        (other, _) => other,
    };
    let primary = match settled {
        Ok(RunOutcome::Submitted(value)) => Ok(value),
        Ok(RunOutcome::Cancelled) => Err(LiveSessionError::Cancelled),
        Ok(RunOutcome::InputEnded) => Err(LiveSessionError::InputEnded),
        Ok(RunOutcome::Signalled(signal)) => Err(LiveSessionError::Signalled(signal)),
        Err(error) => Err(LiveSessionError::TerminalIo(error)),
    };
    let mut failures = Vec::new();
    if let Err(source) = clear {
        failures.push(CleanupFailure::new(CleanupStage::Renderer, source));
    }
    collect_cleanup(&mut failures, screen_cleanup);
    collect_cleanup(&mut failures, raw_cleanup);
    collect_cleanup(&mut failures, signal_cleanup);

    if failures.is_empty() {
        primary
    } else {
        Err(LiveSessionError::Cleanup {
            primary:  primary.err().map(Box::new),
            failures: CleanupFailures::new(failures),
        })
    }
}

fn collect_cleanup(failures: &mut Vec<CleanupFailure>, result: Result<(), CleanupFailures>) {
    if let Err(failed) = result {
        failures.extend(failed.into_failures());
    }
}

fn with_cleanup(primary: LiveSessionError, failures: Vec<CleanupFailure>) -> LiveSessionError {
    if failures.is_empty() {
        primary
    } else {
        LiveSessionError::Cleanup {
            primary:  Some(Box::new(primary)),
            failures: CleanupFailures::new(failures),
        }
    }
}

#[cfg(test)]
mod tests {
    use bang_core::widgets::{
        Select,
        TextInput,
    };
    use screw::{
        CursorVisibility,
        Renderer,
    };

    #[test]
    fn the_terminal_cursor_follows_the_widget_that_places_one() {
        let input = TextInput::new("search")
            .with_prompt("search: ")
            .with_placeholder("type to filter");
        let list = Select::new("results", Vec::<String>::new());
        let mut renderer =
            Renderer::new(Vec::new()).cursor_visibility(CursorVisibility::FromSurface);

        renderer.draw(&input).unwrap();
        renderer.draw(&list).unwrap();
        let output = renderer.into_inner();

        let show = output
            .windows(b"\x1b[?25h".len())
            .position(|part| part == b"\x1b[?25h")
            .expect("text input should show the terminal cursor");
        let hide = output
            .windows(b"\x1b[?25l".len())
            .position(|part| part == b"\x1b[?25l")
            .expect("cursorless widget should hide the terminal cursor");
        assert!(show < hide);
    }
}
