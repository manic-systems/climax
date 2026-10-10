// SPDX-License-Identifier: EUPL-1.2

use std::{error, fmt, io};

/// The result type used across `climax`, with [`Error`] as the error.
pub type Result<T> = std::result::Result<T, Error>;

/// A stable, high-level category for an application error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Command-line arguments could not be parsed.
    Parse,
    /// Terminal output could not be rendered or written.
    Output,
    /// Filesystem or other general I/O failed.
    Io,
    /// An interactive operation failed.
    Interactive,
    /// The user cancelled an operation, or a signal interrupted a prompt.
    ///
    /// [`Error::signal`] tells the two apart.
    Cancelled,
    /// Input ended before an operation completed.
    InputEnded,
    /// Interactive input is unavailable under the current terminal policy.
    InteractionUnavailable,
    /// Another prompt already owns the interactive terminal.
    InteractionBusy,
    /// An application-owned source error.
    Application,
    /// An application-defined error.
    Message,
}

/// An error reported through the `climax` application facade.
///
/// Dependency-specific errors are retained as opaque sources instead of being
/// exposed as variants in the facade contract.
///
/// `Debug` prints the same human-readable report a person wants from
/// `fn main() -> climax::Result<()>`, the message followed by any source or
/// related error text the message does not already carry.
pub struct Error {
    kind: ErrorKind,
    message: String,
    pub(crate) signal: Option<i32>,
    exit_code: Option<u8>,
    source: Option<Box<dyn error::Error + Send + Sync + 'static>>,
    related: Vec<Self>,
}

impl Error {
    #[must_use]
    /// An error carrying only a message, of kind [`ErrorKind::Message`].
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Message,
            message: message.into(),
            signal: None,
            exit_code: None,
            source: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    /// A cancellation, of kind [`ErrorKind::Cancelled`].
    ///
    /// `climax::main` treats it like a cancelled prompt, so it exits 130 and
    /// prints nothing. Return it when the user declines to continue.
    ///
    /// ```
    /// use climax::{Error, ErrorKind};
    ///
    /// let error = Error::cancelled();
    /// assert_eq!(error.kind(), ErrorKind::Cancelled);
    /// assert_eq!(error.exit_code(), None);
    /// ```
    pub fn cancelled() -> Self {
        Self {
            kind: ErrorKind::Cancelled,
            message: "cancelled".to_owned(),
            signal: None,
            exit_code: None,
            source: None,
            related: Vec::new(),
        }
    }

    #[must_use]
    /// Wrap an application error as the source, of kind [`ErrorKind::Application`].
    /// The message is the source's own text.
    pub fn application(source: impl error::Error + Send + Sync + 'static) -> Self {
        Self::with_source(ErrorKind::Application, source)
    }

    #[must_use]
    /// Wrap an application error under a leading message, so the text reads
    /// `message: source`. The kind is [`ErrorKind::Application`].
    pub fn application_context(
        message: impl Into<String>,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        let message = format!("{}: {source}", message.into());
        Self {
            kind: ErrorKind::Application,
            message,
            signal: None,
            exit_code: None,
            source: Some(Box::new(source)),
            related: Vec::new(),
        }
    }

    #[must_use]
    /// Choose the process exit code `climax::main` and `climax::main_with` use
    /// for this error, whatever its kind.
    ///
    /// A cancellation with an explicit code stays silent, and any other kind
    /// is still reported on stderr.
    ///
    /// ```
    /// let error = climax::Error::message("nothing to do").with_exit_code(3);
    /// assert_eq!(error.exit_code(), Some(3));
    /// ```
    pub const fn with_exit_code(mut self, code: u8) -> Self {
        self.exit_code = Some(code);
        self
    }

    #[must_use]
    /// The exit code chosen with [`Error::with_exit_code`], if any.
    pub const fn exit_code(&self) -> Option<u8> {
        self.exit_code
    }

    #[must_use]
    /// The category of this error.
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[must_use]
    /// The retained source with its `Send` and `Sync` bounds, which
    /// `std::error::Error::source` erases.
    pub fn source_error(&self) -> Option<&(dyn error::Error + Send + Sync + 'static)> {
        self.source.as_deref()
    }

    /// The number of the signal that interrupted a prompt, when this is a
    /// [`ErrorKind::Cancelled`] error raised by one.
    ///
    /// `climax::main` exits with 128 plus this number, the status a shell
    /// reports for a process killed by that signal. A plain cancellation has no
    /// signal and exits with 130.
    #[must_use]
    pub const fn signal(&self) -> Option<i32> {
        self.signal
    }

    /// Additional failures reported alongside the primary one, from cleanup or
    /// from a second output stream. The original error remains the primary
    /// kind and source.
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
            exit_code: None,
            source: Some(Box::new(source)),
            related: Vec::new(),
        }
    }
}

impl Error {
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
            .map(|source| source as &(dyn error::Error + 'static))
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
            _ => ErrorKind::Interactive,
        };
        let signal = value.signal().map(bang::terminal::Signal::as_raw);
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

/// The teardown failures a signalled or cancelled session also hit, which
/// bang keeps only inside the opaque source of its error.
#[cfg(feature = "interactive")]
fn cleanup_errors(source: Option<&(dyn error::Error + 'static)>) -> Vec<Error> {
    source
        .and_then(|source| source.downcast_ref::<bang::advanced::LiveSessionError>())
        .map_or(&[][..], bang::advanced::LiveSessionError::cleanup_failures)
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

impl From<Box<dyn error::Error + Send + Sync + 'static>> for Error {
    fn from(value: Box<dyn error::Error + Send + Sync + 'static>) -> Self {
        Self::with_source(ErrorKind::Application, BoxedSource(value))
    }
}

/// An `anyhow::Error` becomes the source of an [`ErrorKind::Application`] error
/// with the anyhow message as its text. The whole anyhow chain is kept, so it
/// prints under `Caused by:`.
///
/// ```
/// use anyhow::Context as _;
/// use climax::prelude::*;
///
/// fn count(text: &str) -> climax::Result<u32> {
///     Ok(text.parse::<u32>().context("reading the count")?)
/// }
///
/// let error = count("seven").unwrap_err();
/// assert_eq!(error.kind(), ErrorKind::Application);
/// assert_eq!(error.to_string(), "reading the count");
/// ```
#[cfg(feature = "anyhow")]
impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Self::with_source(ErrorKind::Application, AnyhowSource(value))
    }
}

#[cfg(feature = "anyhow")]
struct AnyhowSource(anyhow::Error);

#[cfg(feature = "anyhow")]
impl fmt::Debug for AnyhowSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[cfg(feature = "anyhow")]
impl fmt::Display for AnyhowSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(feature = "anyhow")]
impl error::Error for AnyhowSource {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        error::Error::source(&*self.0)
    }
}

struct BoxedSource(Box<dyn error::Error + Send + Sync + 'static>);

impl fmt::Debug for BoxedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for BoxedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl error::Error for BoxedSource {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.0.source()
    }
}

/// Conversions from a foreign `Result` into a [`Result`] with an application
/// [`Error`].
///
/// Implemented for every `Result<T, E>` whose error is a `Send + Sync`
/// `std::error::Error`. Import it by name, since the prelude leaves it out to
/// avoid clashing with `anyhow::Context`. The error becomes the source of an
/// [`ErrorKind::Application`] error.
///
/// ```
/// use climax::{ResultExt as _, prelude::*};
///
/// fn count(text: &str) -> climax::Result<u32> {
///     let count = text.parse::<u32>().context("reading the count")?;
///     Ok(count)
/// }
///
/// assert_eq!(count("7").unwrap(), 7);
/// let error = count("seven").unwrap_err();
/// assert_eq!(error.kind(), ErrorKind::Application);
/// assert_eq!(
///     error.to_string(),
///     "reading the count: invalid digit found in string",
/// );
/// ```
pub trait ResultExt<T> {
    /// Turn the error into an application error that reads `message: source`
    /// and keeps the original error as its source.
    ///
    /// # Errors
    ///
    /// Returns the converted error when `self` is an `Err`.
    fn context(self, message: impl Into<String>) -> Result<T>;

    /// Turn the error into an application error carrying the original error's
    /// text and keeping it as the source.
    ///
    /// ```
    /// use climax::{ResultExt as _, prelude::*};
    ///
    /// let error = "x".parse::<u8>().app_err().unwrap_err();
    /// assert_eq!(error.kind(), ErrorKind::Application);
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the converted error when `self` is an `Err`.
    fn app_err(self) -> Result<T>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: error::Error + Send + Sync + 'static,
{
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|source| Error::application_context(message, source))
    }

    fn app_err(self) -> Result<T> {
        self.map_err(Error::application)
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
    fn context_and_boxed_errors_become_application_errors() {
        let failed = "x".parse::<u8>().context("reading the count").unwrap_err();
        assert_eq!(failed.kind(), ErrorKind::Application);
        assert_eq!(failed.to_string(), "reading the count: invalid digit found in string");
        assert!(failed.source_error().is_some());

        let bare = "x".parse::<u8>().app_err().unwrap_err();
        assert_eq!(bare.to_string(), "invalid digit found in string");

        let boxed: Box<dyn error::Error + Send + Sync> = Box::new(Layered(io::Error::other("disk full")));
        let converted = Error::from(boxed);
        assert_eq!(converted.kind(), ErrorKind::Application);
        assert_eq!(format!("{converted:?}"), "could not save\n\nCaused by:\n    disk full");
    }

    #[cfg(feature = "anyhow")]
    #[test]
    fn anyhow_errors_keep_their_context_chain_as_the_source() {
        fn read() -> Result<u8> {
            let value = anyhow::Context::context("x".parse::<u8>(), "reading the count")?;
            Ok(value)
        }

        let error = read().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Application);
        assert_eq!(error.to_string(), "reading the count");
        assert_eq!(
            format!("{error:?}"),
            "reading the count\n\nCaused by:\n    invalid digit found in string",
        );
        let source = error.source_error().expect("the anyhow error is kept");
        assert_eq!(source.to_string(), "reading the count");
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
        use bang::{advanced::LiveSessionError, terminal::{CleanupFailure, CleanupFailures, CleanupStage, Signal}};

        let source = LiveSessionError::Cleanup {
            primary: Some(Box::new(LiveSessionError::Signalled(Signal::TERM))),
            failures: CleanupFailures::new(vec![
                CleanupFailure::new(CleanupStage::Screen, io::Error::other("no tty")),
                CleanupFailure::new(CleanupStage::RawMode, io::Error::other("EIO")),
            ]),
        };
        let related = cleanup_errors(Some(&source));
        assert_eq!(related.len(), 2);
        assert_eq!(related[0].to_string(), "terminal cleanup failed, Screen: no tty");
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
        assert_eq!(format!("{error:?}"), "could not save\n\nCaused by:\n    disk full");
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
