// SPDX-License-Identifier: EUPL-1.2

use core::{
    error,
    fmt,
    result,
};
use std::io;

#[cfg(feature = "interactive")]
use bang::{
    advanced::LiveSessionError,
    terminal::Signal,
};

/// result type used across `climax`, with [`Error`] as the error
pub type Result<T> = result::Result<T, Error>;

/// stable, high-level category of an application error
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// arguments failed to parse
    Parse,
    /// terminal output failed to render or write
    Output,
    /// filesystem or other i/o failure
    Io,
    /// an interactive operation failed
    Interactive,
    /// the user cancelled, or a signal interrupted a prompt
    ///
    /// [`Error::signal`] tells the two apart
    Cancelled,
    /// input ended before an operation completed
    InputEnded,
    /// interactive input is unavailable under the terminal policy
    InteractionUnavailable,
    /// another prompt owns the interactive terminal
    InteractionBusy,
    /// error with an application-owned source
    Application,
    /// application-defined message
    Message,
}

/// error reported through the `climax` facade
///
/// dependency errors are kept as opaque sources, not variants
///
/// `Debug` prints the report `fn main() -> climax::Result<()>` should show, the
/// message followed by any source or related error text it does not already
/// carry
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "the output module sets the signal when it relays an interrupted error"
)]
pub struct Error {
    kind:              ErrorKind,
    message:           String,
    pub(crate) signal: Option<i32>,
    source:            Option<Box<dyn error::Error + Send + Sync + 'static>>,
    related:           Vec<Self>,
}

impl Error {
    #[must_use]
    /// error with only a message, of kind [`ErrorKind::Message`]
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            kind:    ErrorKind::Message,
            message: message.into(),
            signal:  None,
            source:  None,
            related: Vec::new(),
        }
    }

    #[must_use]
    /// wraps `source` as an [`ErrorKind::Application`] error with its text as
    /// the message
    pub fn application(source: impl error::Error + Send + Sync + 'static) -> Self {
        Self::with_source(ErrorKind::Application, source)
    }

    #[must_use]
    /// like [`Error::application`], with the text reading `message: source`
    pub fn application_context(
        message: impl Into<String>,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        let combined = format!("{}: {source}", message.into());
        Self {
            kind:    ErrorKind::Application,
            message: combined,
            signal:  None,
            source:  Some(Box::new(source)),
            related: Vec::new(),
        }
    }

    #[must_use]
    /// category of this error
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[must_use]
    /// retained source, keeping the `Send` and `Sync` bounds
    /// `std::error::Error::source` erases
    pub fn source_error(&self) -> Option<&(dyn error::Error + Send + Sync + 'static)> {
        self.source.as_deref()
    }

    /// number of the signal that interrupted a prompt, for a
    /// [`ErrorKind::Cancelled`] error raised by one
    ///
    /// `climax::main` exits with 128 plus this number, and a plain cancellation
    /// exits with 130
    #[must_use]
    pub const fn signal(&self) -> Option<i32> {
        self.signal
    }

    /// failures reported alongside the primary one, from cleanup or a second
    /// output stream
    #[must_use]
    pub fn related_errors(&self) -> &[Self] {
        &self.related
    }

    pub(crate) fn with_related(mut self, other: Self) -> Self {
        self.message = format!("{}, and {other}", self.message);
        self.related.push(other);
        self
    }

    pub(crate) fn with_source(
        kind: ErrorKind,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message: source.to_string(),
            signal: None,
            source: Some(Box::new(source)),
            related: Vec::new(),
        }
    }

    fn chain_texts(&self) -> Vec<String> {
        let mut texts = Vec::new();
        let mut collect = |error: &Self| {
            let mut source = error::Error::source(error);
            while let Some(cause) = source {
                texts.push(cause.to_string());
                source = cause.source();
            }
        };
        collect(self);
        self.related.iter().for_each(&mut collect);
        texts.retain(|text| !self.message.contains(text.as_str()));
        texts.dedup();
        texts
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        let texts = self.chain_texts();
        if !texts.is_empty() {
            f.write_str("\n\nCaused by:")?;
            for text in texts {
                write!(f, "\n    {text}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| -> &(dyn error::Error + 'static) { source })
    }
}

#[cfg(feature = "parse")]
impl From<pound::Error> for Error {
    fn from(value: pound::Error) -> Self {
        Self::with_source(ErrorKind::Parse, value)
    }
}

#[cfg(feature = "interactive")]
impl From<bang::Error> for Error {
    fn from(value: bang::Error) -> Self {
        let kind = match value.kind() {
            bang::ErrorKind::Cancelled | bang::ErrorKind::Interrupted => ErrorKind::Cancelled,
            bang::ErrorKind::InputEnded => ErrorKind::InputEnded,
            bang::ErrorKind::InteractionUnavailable => ErrorKind::InteractionUnavailable,
            bang::ErrorKind::InteractionBusy => ErrorKind::InteractionBusy,
            bang::ErrorKind::InvalidConfiguration
            | bang::ErrorKind::Terminal
            | bang::ErrorKind::UnexpectedValue
            | _ => ErrorKind::Interactive,
        };
        let signal = value.signal().map(Signal::as_raw);
        let cleanup = if kind == ErrorKind::Cancelled {
            cleanup_errors(error::Error::source(&value))
        } else {
            Vec::new()
        };
        let mut error = Self::with_source(kind, value);
        error.signal = signal;
        cleanup.into_iter().fold(error, Self::with_related)
    }
}

/// teardown failures of a signalled or cancelled session, which bang keeps only
/// inside the opaque source of its error
#[cfg(feature = "interactive")]
fn cleanup_errors(source: Option<&(dyn error::Error + 'static)>) -> Vec<Error> {
    source
        .and_then(|inner| inner.downcast_ref::<LiveSessionError>())
        .map_or(&[][..], LiveSessionError::cleanup_failures)
        .iter()
        .map(|failure| {
            Error::with_source(
                ErrorKind::Interactive,
                io::Error::other(format!("terminal cleanup failed, {failure}")),
            )
        })
        .collect()
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::with_source(ErrorKind::Io, value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::message(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::message(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_errors_have_no_dependency_source() {
        let error = Error::message("boom");
        assert_eq!(error.kind(), ErrorKind::Message);
        assert_eq!(error.to_string(), "boom");
        assert!(error.source_error().is_none());
    }

    #[test]
    fn dependency_errors_are_opaque_sources() {
        let error = Error::from(io::Error::other("closed"));
        assert_eq!(error.kind(), ErrorKind::Io);
        assert_eq!(error.to_string(), "closed");
        assert!(error.source_error().is_some());
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn cleanup_failures_behind_an_interruption_become_related_errors() {
        use bang::{
            advanced::LiveSessionError,
            terminal::{
                CleanupFailure,
                CleanupFailures,
                CleanupStage,
                Signal,
            },
        };

        let source = LiveSessionError::Cleanup {
            primary:  Some(Box::new(LiveSessionError::Signalled(Signal::TERM))),
            failures: CleanupFailures::new(vec![
                CleanupFailure::new(CleanupStage::Screen, io::Error::other("no tty")),
                CleanupFailure::new(CleanupStage::RawMode, io::Error::other("EIO")),
            ]),
        };
        let related = cleanup_errors(Some(&source));
        assert_eq!(related.len(), 2);
        assert_eq!(
            related[0].to_string(),
            "terminal cleanup failed, screen failed, no tty"
        );
        assert!(cleanup_errors(Some(&LiveSessionError::Cancelled)).is_empty());
    }

    #[derive(Debug)]
    struct Layered(io::Error);

    impl fmt::Display for Layered {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("could not save")
        }
    }

    impl error::Error for Layered {
        fn source(&self) -> Option<&(dyn error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn debug_prints_the_message_and_the_unseen_source_chain() {
        let error = Error::application(Layered(io::Error::other("disk full")));
        assert_eq!(
            format!("{error:?}"),
            "could not save\n\nCaused by:\n    disk full"
        );
        assert_eq!(format!("{:?}", Error::message("boom")), "boom");
        let context = Error::application_context("cannot query", io::Error::other("closed"));
        assert_eq!(format!("{context:?}"), "cannot query: closed");
    }

    #[test]
    fn debug_includes_related_errors_and_their_sources() {
        let error = Error::message("primary")
            .with_related(Error::application(Layered(io::Error::other("no tty"))));
        assert_eq!(
            format!("{error:?}"),
            "primary, and could not save\n\nCaused by:\n    no tty",
        );
    }

    #[test]
    fn application_context_preserves_its_source() {
        let error = Error::application_context("cannot query history", io::Error::other("closed"));
        assert_eq!(error.kind(), ErrorKind::Application);
        assert_eq!(error.to_string(), "cannot query history: closed");
        assert!(error.source_error().is_some());
    }
}
