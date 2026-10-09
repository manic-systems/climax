// SPDX-License-Identifier: EUPL-1.2

use std::{error, fmt, io};

use bang_terminal::Signal;

use crate::live::LiveSessionError;

/// Broad failure category for a prompt interaction.
///
/// Typed prompts resolve [`ErrorKind::Cancelled`] to
/// [`crate::PromptOutcome::Leave`] themselves, so it only reaches a caller
/// from [`crate::advanced`] entry points.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The user cancelled the interaction.
    Cancelled,
    /// Input ended before the prompt was submitted.
    InputEnded,
    /// The process-wide terminal signal handlers are already claimed, by a
    /// live prompt on another terminal handle or by the application's own
    /// signal guard. Prompts on the default driver never see this, since they
    /// wait for each other on the stdin lock instead.
    InteractionBusy,
    /// The driver's terminal is not interactive, for example when stdin is a
    /// pipe or `TERM` is `dumb`.
    InteractionUnavailable,
    /// The prompt was built with settings that cannot be run, such as no
    /// choices or a reserved review action key.
    InvalidConfiguration,
    /// A signal ended the interaction. The terminal is already restored and
    /// the process is still running. [`Error::signal`] names the signal and the
    /// application decides whether to exit, conventionally with 128 plus its
    /// number.
    Interrupted,
    /// Terminal setup, rendering or cleanup failed. The error's source holds
    /// the cause.
    Terminal,
    /// A widget returned a value of a shape the prompt did not expect. This
    /// guards an internal invariant and is not expected in normal use.
    UnexpectedValue,
}

/// Error returned by a Bang prompt.
///
/// Implementation-specific errors remain available through
/// [`error::Error::source`] without becoming part of Bang's public data model.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    signal: Option<Signal>,
    source: Option<Box<dyn error::Error + Send + Sync>>,
}

impl Error {
    /// The failure category.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The signal that interrupted the prompt, set when [`Self::kind`] is
    /// [`ErrorKind::Interrupted`].
    #[must_use]
    pub const fn signal(&self) -> Option<Signal> {
        self.signal
    }

    pub(crate) fn unexpected(expected: &'static str) -> Self {
        Self {
            kind: ErrorKind::UnexpectedValue,
            message: format!("prompt returned an unexpected value; expected {expected}"),
            signal: None,
            source: None,
        }
    }

    pub(crate) fn invalid_configuration(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidConfiguration,
            message: message.into(),
            signal: None,
            source: None,
        }
    }

    pub(crate) fn from_live(error: LiveSessionError) -> Self {
        let primary = error.primary().unwrap_or(&error);
        let signal = match primary {
            LiveSessionError::Signalled(signal) => Some(*signal),
            _ => None,
        };
        let kind = if signal.is_some() {
            ErrorKind::Interrupted
        } else if !error.cleanup_failures().is_empty() {
            ErrorKind::Terminal
        } else {
            match primary {
                LiveSessionError::Unavailable => ErrorKind::InteractionUnavailable,
                LiveSessionError::Cancelled => ErrorKind::Cancelled,
                LiveSessionError::InputEnded => ErrorKind::InputEnded,
                LiveSessionError::Signals(source)
                    if source.kind() == io::ErrorKind::AlreadyExists =>
                {
                    ErrorKind::InteractionBusy
                },
                _ => ErrorKind::Terminal,
            }
        };
        let message = match kind {
            ErrorKind::InteractionUnavailable => {
                "interactive terminal input is unavailable".to_owned()
            },
            ErrorKind::InteractionBusy => {
                "terminal signal handlers are already claimed".to_owned()
            },
            ErrorKind::Cancelled => "prompt was cancelled".to_owned(),
            ErrorKind::InputEnded => "input ended before the prompt was submitted".to_owned(),
            ErrorKind::Interrupted => error.to_string(),
            _ => "terminal failure".to_owned(),
        };
        Self {
            kind,
            message,
            signal,
            source: Some(Box::new(error)),
        }
    }

    /// A driver built on [`crate::advanced::interaction_from_runner`] uses this
    /// to resolve a prompt to [`crate::PromptOutcome::Leave`] the same way a
    /// live session's own cancel does.
    #[must_use]
    pub fn cancelled() -> Self {
        Self {
            kind: ErrorKind::Cancelled,
            message: "prompt was cancelled".to_owned(),
            signal: None,
            source: None,
        }
    }

    /// A driver built on [`crate::advanced::interaction_from_runner`] uses this
    /// to report that input ended before the prompt was submitted.
    #[must_use]
    pub fn input_ended() -> Self {
        Self {
            kind: ErrorKind::InputEnded,
            message: "input ended before the prompt was submitted".to_owned(),
            signal: None,
            source: None,
        }
    }

    /// A driver reports that another interaction already owns the terminal.
    #[must_use]
    pub fn interaction_busy() -> Self {
        Self {
            kind: ErrorKind::InteractionBusy,
            message: "another interaction already owns the terminal".to_owned(),
            signal: None,
            source: None,
        }
    }

    /// A driver reports that its terminal is not interactive.
    #[must_use]
    pub fn interaction_unavailable() -> Self {
        Self {
            kind: ErrorKind::InteractionUnavailable,
            message: "interactive terminal input is unavailable".to_owned(),
            signal: None,
            source: None,
        }
    }

    /// Preserve a terminal lifecycle or rendering failure at the interaction
    /// boundary. The display text includes `source`.
    #[must_use]
    pub fn terminal(source: impl error::Error + Send + Sync + 'static) -> Self {
        Self {
            kind: ErrorKind::Terminal,
            message: "terminal failure".to_owned(),
            signal: None,
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let (ErrorKind::Terminal, Some(source)) = (self.kind, &self.source) {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn error::Error + 'static))
    }
}

/// A result whose error is a Bang [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use std::io;

    use bang_terminal::{CleanupFailure, CleanupFailures, CleanupStage, Signal};

    use super::{Error, ErrorKind, LiveSessionError};

    #[test]
    fn a_signal_is_an_interruption_with_its_cause() {
        let error = Error::from_live(LiveSessionError::Signalled(Signal::INT));

        assert_eq!(error.kind(), ErrorKind::Interrupted);
        assert_eq!(error.signal(), Some(Signal::INT));
        assert_eq!(error.to_string(), "interrupted by signal SIGINT");
    }

    #[test]
    fn a_signal_outranks_a_cleanup_failure() {
        let error = Error::from_live(LiveSessionError::Cleanup {
            primary: Some(Box::new(LiveSessionError::Signalled(Signal::TERM))),
            failures: CleanupFailures::new(vec![CleanupFailure::new(
                CleanupStage::RawMode,
                io::Error::from_raw_os_error(5),
            )]),
        });

        assert_eq!(error.kind(), ErrorKind::Interrupted);
        assert_eq!(error.signal(), Some(Signal::TERM));
    }

    #[test]
    fn claimed_signal_handlers_are_busy() {
        let error = Error::from_live(LiveSessionError::Signals(io::Error::from(
            io::ErrorKind::AlreadyExists,
        )));

        assert_eq!(error.kind(), ErrorKind::InteractionBusy);
    }

    #[test]
    fn a_terminal_error_displays_its_cause() {
        let error = Error::terminal(io::Error::other("pty closed"));

        assert_eq!(error.to_string(), "terminal failure: pty closed");
    }

    #[test]
    fn a_cleanup_failure_after_cancel_is_a_terminal_error() {
        let error = Error::from_live(LiveSessionError::Cleanup {
            primary: Some(Box::new(LiveSessionError::Cancelled)),
            failures: CleanupFailures::new(vec![CleanupFailure::new(
                CleanupStage::RawMode,
                io::Error::from_raw_os_error(5),
            )]),
        });
        assert_eq!(error.kind(), ErrorKind::Terminal);
    }
}
