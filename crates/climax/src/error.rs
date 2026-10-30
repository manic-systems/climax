// SPDX-License-Identifier: EUPL-1.2

use core::{
    error,
    fmt,
    iter,
    result,
};
use std::io;

#[cfg(feature = "interactive")]
use bang::{
    advanced::LiveSessionError,
    terminal::Signal,
};

/// result type used across `climax`, with [`Error`] as the error
pub type Result<T, E = Error> = result::Result<T, E>;

/// stable, high-level category of an application error
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// arguments failed to parse
    Parse,
    /// terminal output failed to render or write
    Output,
    /// filesystem or other i/o failure, also an application error whose source
    /// chain holds one
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
    exit_code:         Option<u8>,
    source:            Option<Box<dyn error::Error + Send + Sync + 'static>>,
    related:           Vec<Self>,
}

impl Error {
    #[must_use]
    /// error with only a message, of kind [`ErrorKind::Message`]
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            kind:      ErrorKind::Message,
            message:   message.into(),
            signal:    None,
            exit_code: None,
            source:    None,
            related:   Vec::new(),
        }
    }

    #[must_use]
    /// cancellation, of kind [`ErrorKind::Cancelled`]
    ///
    /// `climax::main` treats it like a cancelled prompt, exiting 130 and
    /// printing nothing
    pub fn cancelled() -> Self {
        Self {
            kind:      ErrorKind::Cancelled,
            message:   "cancelled".to_owned(),
            signal:    None,
            exit_code: None,
            source:    None,
            related:   Vec::new(),
        }
    }

    #[must_use]
    /// wraps `source` as an [`ErrorKind::Application`] error with its text as
    /// the message, or as [`ErrorKind::Io`] when an `io::Error` is in its chain
    pub fn application(source: impl error::Error + Send + Sync + 'static) -> Self {
        Self::with_source(application_kind(&source), source)
    }

    #[must_use]
    /// like [`Error::application`], with the text reading `message: source`
    pub fn application_context(
        message: impl Into<String>,
        source: impl error::Error + Send + Sync + 'static,
    ) -> Self {
        let combined = format!("{}: {source}", message.into());
        Self {
            kind:      application_kind(&source),
            message:   combined,
            signal:    None,
            exit_code: None,
            source:    Some(Box::new(source)),
            related:   Vec::new(),
        }
    }

    #[must_use]
    /// exit code `climax::main` and `climax::main_with` use for this error
    ///
    /// a cancellation with an explicit code stays silent, any other kind is
    /// still reported on stderr, and a code of 0 is stored as 1
    ///
    /// ```
    /// let error = climax::Error::message("nothing to do").with_exit_code(3);
    /// assert_eq!(error.exit_code(), Some(3));
    /// assert_eq!(error.with_exit_code(0).exit_code(), Some(1));
    /// ```
    pub const fn with_exit_code(mut self, code: u8) -> Self {
        self.exit_code = Some(if code == 0 { 1 } else { code });
        self
    }

    #[must_use]
    /// exit code set with [`Error::with_exit_code`], if any
    pub const fn exit_code(&self) -> Option<u8> {
        self.exit_code
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

    pub(crate) fn is_closed_pipe(&self) -> bool {
        matches!(self.kind, ErrorKind::Output | ErrorKind::Io)
            && causes(self)
                .filter_map(|cause| cause.downcast_ref::<io::Error>())
                .any(|cause| cause.kind() == io::ErrorKind::BrokenPipe)
    }

    pub(crate) fn report(&self) -> String {
        let mut report = format!("error: {self}");
        for text in self.chain_texts() {
            report.push_str("\n  caused by: ");
            report.push_str(&text);
        }
        report
    }

    pub(crate) fn with_related(mut self, other: Self) -> Self {
        self.message = format!("{}, and {other}", self.message);
        self.related.push(other);
        self
    }

    fn inheriting(mut self, inherited: Option<Inherited>) -> Self {
        let Some(Inherited {
            kind,
            signal,
            exit_code,
            cleanup,
            carried,
        }) = inherited
        else {
            return self;
        };
        self.kind = kind;
        self.signal = signal;
        self.exit_code = exit_code;
        self.related = carried;
        cleanup.into_iter().fold(self, Self::with_related)
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
        #[cfg(feature = "parse")]
        if self.kind == ErrorKind::Parse
            && let Some(parse) = self
                .source
                .as_deref()
                .and_then(|source| source.downcast_ref::<pound::Error>())
        {
            let report = parse.render(pound::Term::plain());
            return f.write_str(report.strip_prefix("error: ").unwrap_or(&report));
        }
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
        let inherited = Inherited::from_bang(&value);
        Self::with_source(ErrorKind::Interactive, value).inheriting(Some(inherited))
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

/// how `main` treats a climax or bang error found behind a wrapper
struct Inherited {
    kind:      ErrorKind,
    signal:    Option<i32>,
    exit_code: Option<u8>,
    cleanup:   Vec<Error>,
    carried:   Vec<Error>,
}

impl Inherited {
    fn find<'a>(
        causes: impl IntoIterator<Item = &'a (dyn error::Error + 'static)>,
    ) -> Option<Self> {
        causes.into_iter().find_map(|cause| {
            if let Some(inner) = cause.downcast_ref::<Error>() {
                return Some(Self {
                    kind:      inner.kind,
                    signal:    inner.signal,
                    exit_code: inner.exit_code,
                    cleanup:   Vec::new(),
                    carried:   inner
                        .related
                        .iter()
                        .map(|related| Error::message(related.to_string()))
                        .collect(),
                });
            }
            #[cfg(feature = "interactive")]
            if let Some(inner) = cause.downcast_ref::<bang::Error>() {
                return Some(Self::from_bang(inner));
            }
            None
        })
    }

    #[cfg(feature = "interactive")]
    fn from_bang(value: &bang::Error) -> Self {
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
        let cleanup = if kind == ErrorKind::Cancelled {
            cleanup_errors(error::Error::source(value))
        } else {
            Vec::new()
        };
        Self {
            kind,
            signal: value.signal().map(Signal::as_raw),
            exit_code: None,
            cleanup,
            carried: Vec::new(),
        }
    }
}

fn application_kind(source: &(dyn error::Error + 'static)) -> ErrorKind {
    if causes(source).any(<dyn error::Error>::is::<io::Error>) {
        ErrorKind::Io
    } else {
        ErrorKind::Application
    }
}

fn causes<'a>(
    error: &'a (dyn error::Error + 'static),
) -> impl Iterator<Item = &'a (dyn error::Error + 'static)> {
    iter::successors(Some(error), |cause| cause.source())
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::with_source(ErrorKind::Io, value)
    }
}

impl From<Box<dyn error::Error + Send + Sync + 'static>> for Error {
    fn from(value: Box<dyn error::Error + Send + Sync + 'static>) -> Self {
        let inherited = Inherited::find(causes(&*value));
        Self::with_source(application_kind(&*value), BoxedSource(value)).inheriting(inherited)
    }
}

/// an `anyhow::Error` becomes the source of an [`ErrorKind::Application`]
/// error, and its whole chain is kept so it prints under `Caused by:`
///
/// an [`Error`] or `bang::Error` in the chain keeps its kind, signal and exit
/// code
#[cfg(feature = "anyhow")]
impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        let inherited = Inherited::find(value.chain());
        Self::with_source(application_kind(&*value), AnyhowSource(value)).inheriting(inherited)
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

/// converts a foreign `Result` into a [`Result`], making the error the source
/// of an [`ErrorKind::Application`] error
///
/// an [`Error`] or `bang::Error` among the causes keeps its kind, signal and
/// exit code, so a cancelled prompt stays a cancellation
///
/// implemented for every `Result<T, E>` with a `Send + Sync`
/// `std::error::Error`, and imported by name because the prelude leaves it out
/// to avoid clashing with `anyhow::Context`
///
/// ```
/// use climax::{
///     ResultExt as _,
///     prelude::*,
/// };
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
    /// reads `message: source` and keeps the original error as its source
    ///
    /// # Errors
    ///
    /// fails when `self` is an `Err`
    fn context(self, message: impl Into<String>) -> Result<T>;

    /// carries the original error's text and keeps it as the source
    ///
    /// # Errors
    ///
    /// fails when `self` is an `Err`
    fn app_err(self) -> Result<T>;
}

impl<T, E> ResultExt<T> for result::Result<T, E>
where
    E: error::Error + Send + Sync + 'static,
{
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|source| {
            let inherited = Inherited::find(causes(&source));
            Error::application_context(message, source).inheriting(inherited)
        })
    }

    fn app_err(self) -> Result<T> {
        self.map_err(|source| {
            let inherited = Inherited::find(causes(&source));
            Error::application(source).inheriting(inherited)
        })
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
        assert_eq!(
            failed.to_string(),
            "reading the count: invalid digit found in string"
        );
        assert!(failed.source_error().is_some());

        let bare = "x".parse::<u8>().app_err().unwrap_err();
        assert_eq!(bare.to_string(), "invalid digit found in string");

        let boxed: Box<dyn error::Error + Send + Sync> =
            Box::new(Layered(io::Error::other("disk full")));
        let converted = Error::from(boxed);
        assert_eq!(converted.kind(), ErrorKind::Io);
        assert_eq!(
            format!("{converted:?}"),
            "could not save\n\nCaused by:\n    disk full"
        );
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
    fn a_zero_exit_code_is_stored_as_one() {
        assert_eq!(Error::message("x").with_exit_code(0).exit_code(), Some(1));
        assert_eq!(Error::cancelled().with_exit_code(0).exit_code(), Some(1));
        assert_eq!(
            Error::message("x").with_exit_code(255).exit_code(),
            Some(255)
        );
    }

    #[test]
    fn wrapping_keeps_the_kind_signal_and_exit_code() {
        let mut interrupted = Error::cancelled().with_exit_code(7);
        interrupted.signal = Some(15);
        let wrapped = Err::<(), _>(interrupted).context("confirming").unwrap_err();
        assert_eq!(wrapped.to_string(), "confirming: cancelled");
        assert_eq!(
            (wrapped.kind(), wrapped.signal(), wrapped.exit_code()),
            (ErrorKind::Cancelled, Some(15), Some(7))
        );

        let noisy = Error::cancelled().with_related(Error::message("cleanup failed"));
        let kept = Err::<(), _>(noisy).app_err().unwrap_err();
        assert_eq!(kept.related_errors().len(), 1);

        let piped = Error::from(io::Error::from(io::ErrorKind::BrokenPipe));
        assert!(Err::<(), _>(piped).app_err().unwrap_err().is_closed_pipe());

        let boxed: Box<dyn error::Error + Send + Sync> = Box::new(Error::cancelled());
        assert_eq!(Error::from(boxed).kind(), ErrorKind::Cancelled);

        #[cfg(feature = "interactive")]
        assert_eq!(
            Err::<(), _>(bang::Error::cancelled())
                .app_err()
                .unwrap_err()
                .kind(),
            ErrorKind::Cancelled
        );
        #[cfg(feature = "anyhow")]
        assert_eq!(
            Error::from(anyhow::Error::new(Error::cancelled()).context("confirming")).kind(),
            ErrorKind::Cancelled
        );
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
        let error =
            Error::application_context("cannot query history", "x".parse::<u8>().unwrap_err());
        assert_eq!(error.kind(), ErrorKind::Application);
        assert_eq!(
            error.to_string(),
            "cannot query history: invalid digit found in string"
        );
        assert!(error.source_error().is_some());

        let piped = Layered(io::Error::from(io::ErrorKind::BrokenPipe));
        assert!(
            Err::<(), _>(piped)
                .context("writing")
                .unwrap_err()
                .is_closed_pipe()
        );
    }
}

#[cfg(test)]
mod prelude_result {
    use core::num::ParseIntError;

    use crate::prelude::*;

    fn two_parameters(text: &str) -> Result<u32, ParseIntError> {
        text.parse()
    }

    fn one_parameter(text: &str) -> Result<u32> {
        two_parameters(text).map_err(Error::application)
    }

    #[test]
    fn the_prelude_result_takes_one_or_two_parameters() {
        assert_eq!(one_parameter("4").unwrap(), 4);
        two_parameters("x").unwrap_err();
    }
}
